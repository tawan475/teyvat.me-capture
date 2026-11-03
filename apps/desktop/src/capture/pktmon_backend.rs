// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::Instant;

use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::FusedStream;
use pktmon::filter::{PktMonFilter, TransportProtocol};
use pktmon::{Capture, CapturePath, Packet, PacketPayload};

use crate::capture::dedupe::{DEDUPE_CAPACITY, DEDUPE_WINDOW, Deduper};
use crate::capture::{
    CaptureBackend, CaptureError, CapturedFrame, DUPLICATES_DROPPED, MERGED_DATAGRAMS_SEEN,
    PORT_RANGE, Result, is_merged_datagram,
};

/// Length of an Ethernet II header (destination MAC, source MAC, ethertype).
const ETHERNET_HEADER_LEN: usize = 14;
const ETHERTYPE_IPV4: u16 = 0x0800;
const ETHERTYPE_IPV6: u16 = 0x86dd;

/// The LLC/SNAP header an 802.11 data frame's body opens with when it carries
/// an Ethernet-typed payload: DSAP/SSAP `aa aa`, control `03`, then the OUI
/// (`00 00 00`, or `00 00 f8` for the bridge-tunnel encapsulation of AppleTalk
/// and IPX) and the ethertype.
const LLC_SNAP: [u8; 3] = [0xaa, 0xaa, 0x03];

/// What had to be done to a payload that pktmon did not report as an Ethernet
/// frame. Used only for one-shot diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadNote {
    /// A bare IP packet was given a synthetic Ethernet header.
    WrappedIp,
    /// An 802.11 data frame was rewritten as the Ethernet frame it carries.
    ConvertedWifi,
    /// The payload was handed on unchanged and will most likely fail to parse.
    PassedThrough(&'static str),
}

impl PayloadNote {
    /// Key used to deduplicate warnings, so each surprising layer is reported
    /// once rather than once per packet.
    fn key(&self) -> &'static str {
        match self {
            PayloadNote::WrappedIp => "IP",
            PayloadNote::ConvertedWifi => "WiFi",
            PayloadNote::PassedThrough(kind) => kind,
        }
    }
}

pub struct PktmonBackend {
    stream: Box<dyn FusedStream<Item = Packet> + Unpin + Send>,
    /// Payload kinds already reported, so an unexpected link layer produces one
    /// log line rather than one per packet.
    warned_payload_kinds: HashSet<&'static str>,
    /// pktmon reports each datagram once per point of the stack it passes;
    /// only the first copy is passed on.
    dedupe: Deduper,
    /// Watching for merged datagrams: on the real-time stream, until the first.
    watch_merging: bool,
}

impl PktmonBackend {
    /// `compatible` reads pktmon's ETW path; otherwise its real-time stream,
    /// which drops packets over 9000 bytes (see `MERGED_DATAGRAMS_SEEN`). The
    /// ETW path is only the fallback behind `raw_backend` now: on the PC that
    /// merges packets it still lost 3-6% of the game's pieces, unreported.
    ///
    /// The ETW path is one capture for the whole system: starting it stops any
    /// capture another program had running on it (the `pktmon` command, other
    /// tools), so the game is always captured. The stream gives each program a
    /// session of its own. Neither ever unloads the pktmon driver.
    pub fn new(compatible: bool) -> Result<Self> {
        let path = if compatible {
            CapturePath::Etw
        } else {
            CapturePath::Auto
        };
        tracing::info!(?path, "starting pktmon");
        // Debug builds: `TEYVATME_FAKE_MERGED` acts as if this PC merged the
        // game's packets, to test the switch on one that doesn't.
        #[cfg(debug_assertions)]
        if !compatible && std::env::var_os("TEYVATME_FAKE_MERGED").is_some() {
            MERGED_DATAGRAMS_SEEN.store(true, Ordering::Relaxed);
        }
        let mut capture = Capture::with_path(path).map_err(|e| CaptureError::Capture {
            has_captured: false,
            error: e.into(),
        })?;

        let filter = PktMonFilter {
            name: "UDP Filter".to_string(),
            transport_protocol: Some(TransportProtocol::UDP),
            port: PORT_RANGE.0.into(),
            ..PktMonFilter::default()
        };

        capture
            .add_filter(filter)
            .map_err(|e| CaptureError::Filter(e.into()))?;

        let filter = PktMonFilter {
            name: "UDP Filter".to_string(),
            transport_protocol: Some(TransportProtocol::UDP),
            port: PORT_RANGE.1.into(),
            ..PktMonFilter::default()
        };

        capture
            .add_filter(filter)
            .map_err(|e| CaptureError::Filter(e.into()))?;

        // What pktmon can capture on, once per start: on one PC it delivered
        // only the client's sends, as bare IP packets, and nothing in the
        // log said which adapters its hook was on.
        match capture.data_sources() {
            Ok(sources) => {
                // Adapters and the components pktmon can capture on now;
                // the rest (unbound filters) would be most of the list.
                for source in sources
                    .into_iter()
                    .filter(|source| source.present || source.network_interface)
                {
                    tracing::info!(
                        id = source.id,
                        parent = source.parent_id,
                        present = source.present,
                        interface = source.network_interface,
                        name = %source.name,
                        description = %source.description,
                        "pktmon data source"
                    );
                }
            }
            Err(e) => tracing::info!("pktmon data sources not listed: {e}"),
        }

        // `stream()` starts the pktmon session, so it fails when the session
        // cannot be started. Report that as a capture error instead of
        // panicking inside whatever task owns this backend.
        let stream = capture.stream().map_err(|e| CaptureError::Capture {
            has_captured: false,
            error: e.into(),
        })?;

        Ok(Self {
            stream: Box::new(stream.boxed().fuse()),
            warned_payload_kinds: HashSet::new(),
            dedupe: Deduper::new(DEDUPE_CAPACITY, DEDUPE_WINDOW),
            watch_merging: !compatible,
        })
    }

    fn note_payload(&mut self, note: PayloadNote) {
        let key = note.key();
        if !self.warned_payload_kinds.insert(key) {
            return;
        }

        match note {
            PayloadNote::WrappedIp => tracing::warn!(
                "pktmon reported a bare IP payload instead of an Ethernet frame; wrapping it in a \
                 synthetic Ethernet header so it can be parsed. Further IP payloads are not logged."
            ),
            PayloadNote::ConvertedWifi => tracing::info!(
                "pktmon reported 802.11 frames (a Wi-Fi adapter's own copies); they are read as \
                 the Ethernet frames they carry. Further ones are not logged."
            ),
            PayloadNote::PassedThrough(kind) => tracing::warn!(
                "pktmon reported a {kind} payload instead of an Ethernet frame; passing it through \
                 unchanged, and it will most likely fail to parse. Further {kind} payloads are not \
                 logged."
            ),
        }
    }
}

/// Turn a pktmon payload into the Ethernet frame the packet parser expects.
///
/// Everything downstream (`GameSniffer::receive_packet`) parses these bytes
/// with `SlicedPacket::from_ethernet`, so a payload reported at another layer
/// is misread as a MAC header and silently dropped. A bare IP payload is given
/// a synthetic Ethernet header; anything else is passed through unchanged (the
/// historical behaviour) with a note so the failure is diagnosable.
fn to_ethernet_frame(payload: PacketPayload) -> (Vec<u8>, Option<PayloadNote>) {
    match payload {
        PacketPayload::Ethernet(data) => (data, None),
        PacketPayload::IP(data) => match wrap_ip_in_ethernet(&data) {
            Some(frame) => (frame, Some(PayloadNote::WrappedIp)),
            None => (data, Some(PayloadNote::PassedThrough("IP"))),
        },
        PacketPayload::WiFi(data) => match wifi_to_ethernet(&data) {
            Some(frame) => (frame, Some(PayloadNote::ConvertedWifi)),
            None => (data, Some(PayloadNote::PassedThrough("WiFi"))),
        },
        other => {
            let note = PayloadNote::PassedThrough(payload_kind(&other));
            (other.to_vec().clone(), Some(note))
        }
    }
}

/// Prepend a synthetic Ethernet II header to a bare IP packet.
///
/// The MAC addresses are zeroed (nothing downstream reads them) and the
/// ethertype comes from the IP version nibble. Returns `None` when the buffer
/// does not start with a plausible IPv4 or IPv6 header, in which case the
/// caller should leave the bytes alone rather than corrupt them further.
pub(super) fn wrap_ip_in_ethernet(ip: &[u8]) -> Option<Vec<u8>> {
    let ethertype = match ip.first()? >> 4 {
        4 => ETHERTYPE_IPV4,
        6 => ETHERTYPE_IPV6,
        _ => return None,
    };

    let mut frame = Vec::with_capacity(ETHERNET_HEADER_LEN + ip.len());
    frame.extend_from_slice(&[0u8; 12]);
    frame.extend_from_slice(&ethertype.to_be_bytes());
    frame.extend_from_slice(ip);
    Some(frame)
}

/// The Ethernet II frame an 802.11 data frame carries, or `None` for any
/// other 802.11 frame (management, control, a frame without a body, an
/// encrypted one, an A-MSDU, or a body that is not LLC/SNAP-encapsulated).
///
/// On a Wi-Fi adapter pktmon's copies from the adapter itself and the filters
/// under the 802.11-to-802.3 conversion (nwifi) are the station's own 802.11
/// frames, plaintext (the adapter does the encryption) with an LLC/SNAP body.
/// Only nwifi's copy is Ethernet, and on one PC it never came (2026-11-02), so
/// an inbound packet depended on a single component; these copies are the
/// other three.
///
/// Header: frame control (2), duration (2), three addresses (18), sequence
/// control (2), then a fourth address when both ToDS and FromDS are set, a
/// QoS control word for QoS data subtypes, and an HT control word when the
/// Order flag is set on a QoS frame. Which address is the destination and
/// which the source follows ToDS/FromDS.
pub(super) fn wifi_to_ethernet(frame: &[u8]) -> Option<Vec<u8>> {
    let frame_control = frame.get(0..2)?;
    let (kind, subtype) = ((frame_control[0] >> 2) & 0b11, frame_control[0] >> 4);
    let flags = frame_control[1];
    let (to_ds, from_ds) = (flags & 0b01 != 0, flags & 0b10 != 0);
    let (protected, order) = (flags & 0x40 != 0, flags & 0x80 != 0);
    // Data frames only, and of them only the subtypes that carry a body
    // (0-3 and their QoS forms 8-11; 4-7 and 12-15 are null or reserved).
    if kind != 2 || subtype & 0b0100 != 0 || protected {
        return None;
    }
    let qos = subtype & 0b1000 != 0;

    // Addresses 1-3 at 4, 10 and 16; the sequence control at 22; address 4,
    // when present, at 24.
    let address = |at: usize| frame.get(at..at + 6);
    let (destination, source) = match (to_ds, from_ds) {
        (false, false) => (address(4)?, address(10)?),
        (false, true) => (address(4)?, address(16)?),
        (true, false) => (address(16)?, address(10)?),
        (true, true) => (address(16)?, address(24)?),
    };

    let mut body_at = 24;
    if to_ds && from_ds {
        body_at += 6;
    }
    if qos {
        let qos_control = frame.get(body_at..body_at + 2)?;
        // An A-MSDU packs several subframes; nothing here sends those.
        if qos_control[0] & 0x80 != 0 {
            return None;
        }
        body_at += 2;
        if order {
            body_at += 4;
        }
    }

    let body = frame.get(body_at..)?;
    let snap = body.get(0..8)?;
    if snap[0..3] != LLC_SNAP || (snap[3..6] != [0, 0, 0] && snap[3..6] != [0, 0, 0xf8]) {
        return None;
    }
    let mut ethernet = Vec::with_capacity(ETHERNET_HEADER_LEN + body.len() - 8);
    ethernet.extend_from_slice(destination);
    ethernet.extend_from_slice(source);
    ethernet.extend_from_slice(&snap[6..8]);
    ethernet.extend_from_slice(&body[8..]);
    Some(ethernet)
}

/// Stable, allocation-free label for a payload variant, used in diagnostics.
fn payload_kind(payload: &PacketPayload) -> &'static str {
    match payload {
        PacketPayload::Unknown(_) => "Unknown",
        PacketPayload::Ethernet(_) => "Ethernet",
        PacketPayload::WiFi(_) => "WiFi",
        PacketPayload::IP(_) => "IP",
        PacketPayload::HTTP(_) => "HTTP",
        PacketPayload::TCP(_) => "TCP",
        PacketPayload::UDP(_) => "UDP",
        PacketPayload::ARP(_) => "ARP",
        PacketPayload::ICMP(_) => "ICMP",
        PacketPayload::ESP(_) => "ESP",
        PacketPayload::AH(_) => "AH",
        PacketPayload::L4Payload(_) => "L4Payload",
    }
}

#[async_trait]
impl CaptureBackend for PktmonBackend {
    async fn next_packet(&mut self) -> Result<CapturedFrame> {
        loop {
            // The payload is pulled out of the `select!` before it is converted
            // so the mutable borrow of `self.stream` ends before `self` is
            // reborrowed.
            let payload = futures::select! {
                packet = self.stream.select_next_some() => Some((packet.component_id, packet.payload)),
                complete => None,
            };

            let Some((component, payload)) = payload else {
                return Err(CaptureError::CaptureClosed);
            };

            let (frame, note) = to_ethernet_frame(payload);
            if let Some(note) = note {
                self.note_payload(note);
            }
            if self.dedupe.is_duplicate(&frame, Instant::now()) {
                DUPLICATES_DROPPED.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if self.watch_merging && is_merged_datagram(&frame) {
                self.watch_merging = false;
                tracing::warn!(
                    len = frame.len(),
                    "this PC merges the game's incoming packets (UDP receive offload); pktmon's                      real-time stream drops the merged ones over 9000 bytes"
                );
                MERGED_DATAGRAMS_SEEN.store(true, Ordering::Relaxed);
            }
            return Ok(CapturedFrame {
                data: frame,
                source: Some(component),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ethernet_payloads_pass_through_unchanged_and_silently() {
        let data = vec![0xaa, 0xbb, 0xcc];
        let (frame, note) = to_ethernet_frame(PacketPayload::Ethernet(data.clone()));

        assert_eq!(frame, data);
        assert_eq!(note, None);
    }

    #[test]
    fn ipv4_payloads_gain_a_synthetic_ethernet_header() {
        // Minimal IPv4 header start: version 4, IHL 5.
        let ip = vec![0x45, 0x00, 0x00, 0x1c, 0xde, 0xad];
        let (frame, note) = to_ethernet_frame(PacketPayload::IP(ip.clone()));

        assert_eq!(note, Some(PayloadNote::WrappedIp));
        assert_eq!(frame.len(), ETHERNET_HEADER_LEN + ip.len());
        assert_eq!(&frame[0..12], &[0u8; 12]);
        assert_eq!(&frame[12..14], &ETHERTYPE_IPV4.to_be_bytes());
        assert_eq!(&frame[14..], &ip[..]);
    }

    #[test]
    fn ipv6_payloads_gain_the_ipv6_ethertype() {
        let ip = vec![0x60, 0x00, 0x00, 0x00];
        let (frame, note) = to_ethernet_frame(PacketPayload::IP(ip.clone()));

        assert_eq!(note, Some(PayloadNote::WrappedIp));
        assert_eq!(&frame[12..14], &ETHERTYPE_IPV6.to_be_bytes());
        assert_eq!(&frame[14..], &ip[..]);
    }

    #[test]
    fn unrecognisable_ip_payloads_are_left_alone() {
        // Version nibble 5 is not a real IP version.
        let ip = vec![0x55, 0x00];
        let (frame, note) = to_ethernet_frame(PacketPayload::IP(ip.clone()));

        assert_eq!(frame, ip);
        assert_eq!(note, Some(PayloadNote::PassedThrough("IP")));

        let (frame, note) = to_ethernet_frame(PacketPayload::IP(Vec::new()));
        assert!(frame.is_empty());
        assert_eq!(note, Some(PayloadNote::PassedThrough("IP")));
    }

    #[test]
    fn other_layers_are_passed_through_but_reported() {
        let data = vec![1, 2, 3, 4];
        let (frame, note) = to_ethernet_frame(PacketPayload::WiFi(data.clone()));

        assert_eq!(frame, data);
        assert_eq!(note, Some(PayloadNote::PassedThrough("WiFi")));

        let data = vec![1, 2, 3, 4];
        let (frame, note) = to_ethernet_frame(PacketPayload::ARP(data.clone()));
        assert_eq!(frame, data);
        assert_eq!(note, Some(PayloadNote::PassedThrough("ARP")));
    }

    /// A QoS data frame from the access point (FromDS) as pktmon delivered it
    /// from an Intel Wi-Fi adapter on 2026-11-03 (an NTP reply; the MACs are
    /// made up, same length), and nwifi's Ethernet copy of the same packet.
    fn qos_data_frame_from_ap() -> (Vec<u8>, Vec<u8>) {
        let station = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
        let access_point = [0x02, 0xaa, 0xbb, 0xcc, 0xdd, 0xee];
        let ip = hex(
            "4530004c33b8000067114c12d8ef23040a000002007b56550038c5a7240100ec0000000000000007474f\
             4f47ee94406aaf173dd90000000000000000ee94406aaf173ddaee94406aaf173ddc",
        );
        let mut wifi = vec![0x88, 0x02, 0x30, 0x00];
        wifi.extend_from_slice(&station); // addr1: destination
        wifi.extend_from_slice(&access_point); // addr2: BSSID
        wifi.extend_from_slice(&access_point); // addr3: source
        wifi.extend_from_slice(&[0xe0, 0x01]); // sequence control
        wifi.extend_from_slice(&[0x00, 0x00]); // QoS control
        wifi.extend_from_slice(&[0xaa, 0xaa, 0x03, 0x00, 0x00, 0x00, 0x08, 0x00]);
        wifi.extend_from_slice(&ip);
        let mut ethernet = Vec::new();
        ethernet.extend_from_slice(&station);
        ethernet.extend_from_slice(&access_point);
        ethernet.extend_from_slice(&[0x08, 0x00]);
        ethernet.extend_from_slice(&ip);
        (wifi, ethernet)
    }

    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
        digits
            .chunks(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn a_qos_data_frame_from_the_ap_becomes_nwifis_ethernet_copy() {
        let (wifi, ethernet) = qos_data_frame_from_ap();
        assert_eq!(wifi_to_ethernet(&wifi).as_deref(), Some(&ethernet[..]));

        let (frame, note) = to_ethernet_frame(PacketPayload::WiFi(wifi));
        assert_eq!(frame, ethernet);
        assert_eq!(note, Some(PayloadNote::ConvertedWifi));
    }

    #[test]
    fn the_addresses_follow_to_ds_and_from_ds() {
        let (mut wifi, ethernet) = qos_data_frame_from_ap();
        let station = &ethernet[0..6];
        let access_point = &ethernet[6..12];

        // ToDS (the station's own send): addr1 BSSID, addr2 source, addr3 destination.
        wifi[1] = 0x01;
        wifi[4..10].copy_from_slice(access_point);
        wifi[10..16].copy_from_slice(station);
        wifi[16..22].copy_from_slice(access_point);
        let converted = wifi_to_ethernet(&wifi).unwrap();
        assert_eq!(&converted[0..6], access_point, "destination");
        assert_eq!(&converted[6..12], station, "source");
        assert_eq!(&converted[12..], &ethernet[12..]);

        // Neither (ad hoc): addr1 destination, addr2 source.
        wifi[1] = 0x00;
        wifi[4..10].copy_from_slice(station);
        wifi[10..16].copy_from_slice(access_point);
        let converted = wifi_to_ethernet(&wifi).unwrap();
        assert_eq!(&converted[0..12], &ethernet[0..12]);

        // Both (WDS): a fourth address follows the sequence control; addr3
        // destination, addr4 source.
        let (plain, ethernet) = qos_data_frame_from_ap();
        let mut wds = plain[0..24].to_vec();
        wds[1] = 0x03;
        wds[16..22].copy_from_slice(station);
        wds.extend_from_slice(access_point);
        wds.extend_from_slice(&plain[24..]);
        assert_eq!(wifi_to_ethernet(&wds).unwrap(), ethernet);
    }

    #[test]
    fn non_qos_data_frames_have_no_qos_control_word() {
        let (wifi, ethernet) = qos_data_frame_from_ap();
        let mut plain = wifi.clone();
        plain[0] = 0x08; // subtype 0: Data
        plain.drain(24..26); // no QoS control
        assert_eq!(wifi_to_ethernet(&plain).unwrap(), ethernet);
    }

    #[test]
    fn an_ht_control_word_follows_the_qos_control_when_order_is_set() {
        let (wifi, ethernet) = qos_data_frame_from_ap();
        let mut ht = wifi.clone();
        ht[1] |= 0x80;
        ht.splice(26..26, [0u8; 4]);
        assert_eq!(wifi_to_ethernet(&ht).unwrap(), ethernet);
    }

    #[test]
    fn frames_without_an_ethernet_payload_are_refused() {
        let (wifi, _) = qos_data_frame_from_ap();

        let mut protected = wifi.clone();
        protected[1] |= 0x40;
        assert!(wifi_to_ethernet(&protected).is_none(), "encrypted body");

        let mut management = wifi.clone();
        management[0] = 0x80; // beacon
        assert!(wifi_to_ethernet(&management).is_none());

        let mut control = wifi.clone();
        control[0] = 0xd4; // ACK
        assert!(wifi_to_ethernet(&control).is_none());

        let mut null = wifi.clone();
        null[0] = 0xc8; // QoS Null
        assert!(wifi_to_ethernet(&null).is_none());

        let mut amsdu = wifi.clone();
        amsdu[24] |= 0x80;
        assert!(wifi_to_ethernet(&amsdu).is_none());

        let mut raw = wifi.clone();
        raw[26] = 0x45; // body that is not LLC/SNAP
        assert!(wifi_to_ethernet(&raw).is_none());

        assert!(
            wifi_to_ethernet(&wifi[..30]).is_none(),
            "cut before the SNAP header"
        );
        assert!(wifi_to_ethernet(&[]).is_none());
        assert!(wifi_to_ethernet(&[0x88]).is_none());
    }

    #[test]
    fn wrap_ip_in_ethernet_rejects_non_ip_buffers() {
        assert!(wrap_ip_in_ethernet(&[]).is_none());
        assert!(wrap_ip_in_ethernet(&[0x00]).is_none());
        assert!(wrap_ip_in_ethernet(&[0xf0]).is_none());
    }

    #[test]
    fn payload_kinds_are_distinct_labels() {
        let payloads = [
            PacketPayload::Unknown(Vec::new()),
            PacketPayload::Ethernet(Vec::new()),
            PacketPayload::WiFi(Vec::new()),
            PacketPayload::IP(Vec::new()),
            PacketPayload::HTTP(Vec::new()),
            PacketPayload::TCP(Vec::new()),
            PacketPayload::UDP(Vec::new()),
            PacketPayload::ARP(Vec::new()),
            PacketPayload::ICMP(Vec::new()),
            PacketPayload::ESP(Vec::new()),
            PacketPayload::AH(Vec::new()),
            PacketPayload::L4Payload(Vec::new()),
        ];

        let kinds: HashSet<&'static str> = payloads.iter().map(payload_kind).collect();
        assert_eq!(kinds.len(), payloads.len());
    }
}
