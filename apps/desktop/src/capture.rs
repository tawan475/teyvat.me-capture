// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

// Only pktmon reports copies, so off Windows only the tests build this.
#[cfg(any(windows, test))]
mod dedupe;
#[cfg(any(feature = "pcap", not(windows)))]
mod pcap_backend;
#[cfg(windows)]
mod pktmon_backend;

use std::fmt::{Debug, Display};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};

use anyhow::Error;
use async_trait::async_trait;
use clap::ValueEnum;

pub const PORT_RANGE: (u16, u16) = (22101, 22102);

/// Duplicate copies of a datagram a capture backend dropped, for the stats
/// line. Only pktmon reports copies; see `dedupe`.
pub static DUPLICATES_DROPPED: AtomicU64 = AtomicU64::new(0);

/// Set when pktmon's real-time stream delivered a merged game datagram (see
/// [`is_merged_datagram`]). The monitor takes it and moves capture to pktmon's
/// ETW path: the stream drops every packet over 9000 bytes, and on a PC that
/// merges incoming UDP that was a third of the game's traffic, the login's key
/// included.
pub static MERGED_DATAGRAMS_SEEN: AtomicBool = AtomicBool::new(false);

/// Events and buffers pktmon's ETW session has lost so far (compatible
/// capture), for the stats line; `None` when that session isn't running.
pub fn etw_losses() -> Option<(u32, u32)> {
    #[cfg(windows)]
    return pktmon::etw_session_losses();
    #[cfg(not(windows))]
    None
}

/// The largest IP packet a game server's datagram arrives in on its own. Its
/// KCP packets stay well under this (~1,230 bytes); anything bigger came in
/// one piece only because this PC merged several datagrams into it (UDP
/// receive offload, Windows 11 24H2), since the path between can't carry it.
#[cfg(any(windows, test))]
const LARGEST_SINGLE_IP_PACKET: usize = 1500;

/// Whether `frame` is several of the game server's datagrams merged into one:
/// UDP from a game port in an IP packet bigger than any the path can carry,
/// and not an IP fragment.
#[cfg(any(windows, test))]
pub fn is_merged_datagram(frame: &[u8]) -> bool {
    let Some(ethertype) = frame.get(12..14) else {
        return false;
    };
    let (udp, ip_len) = match u16::from_be_bytes([ethertype[0], ethertype[1]]) {
        0x0800 => {
            let Some(header) = frame.get(14..34) else {
                return false;
            };
            let header_len = usize::from(header[0] & 0x0F) * 4;
            let fragmented = u16::from_be_bytes([header[6], header[7]]) & 0x3FFF != 0;
            if header[9] != 17 || header_len < 20 || fragmented {
                return false;
            }
            let ip_len = usize::from(u16::from_be_bytes([header[2], header[3]]));
            (frame.get(14 + header_len..), ip_len)
        }
        0x86DD => {
            let Some(header) = frame.get(14..54) else {
                return false;
            };
            if header[6] != 17 {
                return false;
            }
            let ip_len = 40 + usize::from(u16::from_be_bytes([header[4], header[5]]));
            (frame.get(54..), ip_len)
        }
        _ => return false,
    };
    let Some(ports) = udp.and_then(|udp| udp.get(0..2)) else {
        return false;
    };
    let source = u16::from_be_bytes([ports[0], ports[1]]);
    (PORT_RANGE.0..=PORT_RANGE.1).contains(&source) && ip_len > LARGEST_SINGLE_IP_PACKET
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum CaptureError {
    Filter(Error),
    Capture { has_captured: bool, error: Error },
    CaptureClosed,
    ChannelClosed,
    SavefileError(Error),
}

impl Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::Filter(e) => write!(f, "Filter error: {}", e),
            CaptureError::Capture {
                has_captured,
                error,
            } => write!(
                f,
                "Capture error (has_captured = {}): {}",
                has_captured, error
            ),
            CaptureError::CaptureClosed => write!(f, "Capture closed"),
            CaptureError::ChannelClosed => write!(f, "Channel closed"),
            CaptureError::SavefileError(e) => write!(f, "Savefile open error: {}", e),
        }
    }
}

pub type Result<T> = std::result::Result<T, CaptureError>;

#[async_trait]
#[allow(clippy::double_must_use)]
pub trait CaptureBackend: Send {
    async fn next_packet(&mut self) -> Result<Vec<u8>>;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
#[allow(unused)]
pub enum BackendType {
    #[cfg(windows)]
    Pktmon,
    Pcap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureSource {
    Device(Option<PathBuf>),
    File(PathBuf),
}

#[cfg(windows)]
pub const DEFAULT_CAPTURE_BACKEND_TYPE: BackendType = BackendType::Pktmon;
#[cfg(not(windows))]
pub const DEFAULT_CAPTURE_BACKEND_TYPE: BackendType = BackendType::Pcap;

/// `compatible` picks pktmon's ETW path over its real-time stream (Windows);
/// see [`MERGED_DATAGRAMS_SEEN`].
#[cfg_attr(not(windows), allow(unused_variables))]
pub fn create_capture(
    backend: BackendType,
    capture_source: CaptureSource,
    compatible: bool,
) -> Result<Box<dyn CaptureBackend>> {
    match backend {
        #[cfg(windows)]
        BackendType::Pktmon => match capture_source {
            CaptureSource::Device(None) => {
                Ok(Box::new(pktmon_backend::PktmonBackend::new(compatible)?))
            }
            _ => Err(CaptureError::Capture {
                has_captured: false,
                error: anyhow::anyhow!("Savefiles are only supported for the pcap backend"),
            }),
        },

        #[cfg(any(feature = "pcap", not(windows)))]
        BackendType::Pcap => Ok(Box::new(pcap_backend::PcapBackend::new(capture_source)?)),
        #[cfg(not(any(feature = "pcap", not(windows))))]
        BackendType::Pcap => Err(CaptureError::Capture {
            has_captured: false,
            error: anyhow::anyhow!(
                "Please enable the pcap feature during build to use the pcap backend",
            ),
        }),

        #[allow(unreachable_patterns)]
        _ => Err(CaptureError::Capture {
            has_captured: false,
            error: anyhow::anyhow!(
                "Capture backend type {:?} not supported on this operating system",
                backend
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An Ethernet frame holding one IPv4 UDP datagram from `source` whose IP
    /// packet claims `ip_len` bytes, with the IP flags/fragment field `frag`.
    fn ipv4_frame(source: u16, ip_len: u16, frag: u16) -> Vec<u8> {
        let mut frame = vec![0u8; 12];
        frame.extend_from_slice(&0x0800u16.to_be_bytes());
        let mut ip = vec![0u8; 20];
        ip[0] = 0x45;
        ip[2..4].copy_from_slice(&ip_len.to_be_bytes());
        ip[6..8].copy_from_slice(&frag.to_be_bytes());
        ip[9] = 17;
        frame.extend_from_slice(&ip);
        frame.extend_from_slice(&source.to_be_bytes());
        frame.extend_from_slice(&[0x12, 0x34, 0, 0, 0, 0]);
        frame
    }

    #[test]
    fn a_game_datagram_too_big_for_the_path_is_merged() {
        // Two of the login reply's 1,228-byte datagrams in one packet.
        assert!(is_merged_datagram(&ipv4_frame(PORT_RANGE.0, 2428, 0)));
        assert!(is_merged_datagram(&ipv4_frame(PORT_RANGE.1, 8428, 0x4000)));
    }

    #[test]
    fn ordinary_and_unrelated_packets_are_not() {
        // A full-size game datagram on its own.
        assert!(!is_merged_datagram(&ipv4_frame(PORT_RANGE.0, 1228, 0)));
        // The client's own traffic, or anything else, however big.
        assert!(!is_merged_datagram(&ipv4_frame(55584, 2428, 0)));
        // An IP fragment says nothing about merging.
        assert!(!is_merged_datagram(&ipv4_frame(PORT_RANGE.0, 2428, 0x2000)));
        assert!(!is_merged_datagram(&ipv4_frame(PORT_RANGE.0, 2428, 0x00B9)));
        // Not UDP, not IP, or too short to tell.
        let mut tcp = ipv4_frame(PORT_RANGE.0, 2428, 0);
        tcp[23] = 6;
        assert!(!is_merged_datagram(&tcp));
        assert!(!is_merged_datagram(&[0u8; 20]));
        assert!(!is_merged_datagram(&[]));
    }

    #[test]
    fn ipv6_counts_its_fixed_header() {
        let mut frame = vec![0u8; 12];
        frame.extend_from_slice(&0x86DDu16.to_be_bytes());
        let mut ip = vec![0u8; 40];
        ip[0] = 0x60;
        ip[4..6].copy_from_slice(&2388u16.to_be_bytes());
        ip[6] = 17;
        frame.extend_from_slice(&ip);
        frame.extend_from_slice(&PORT_RANGE.0.to_be_bytes());
        frame.extend_from_slice(&[0; 6]);
        assert!(is_merged_datagram(&frame));
        frame[18..20].copy_from_slice(&1000u16.to_be_bytes());
        assert!(!is_merged_datagram(&frame));
    }
}
