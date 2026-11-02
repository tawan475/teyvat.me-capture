// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Capture through raw sockets in receive-all mode (`SIO_RCVALL`): what
//! RawCap uses. No driver, no pktmon.
//!
//! Windows' TCP/IP stack copies every IPv4 packet that passes an interface to
//! such a socket, sent ones included (since Windows 7), one copy each, at the
//! IP layer. That sidesteps both of pktmon's losses on a PC whose network card
//! merges the game's incoming datagrams (UDP receive offload): the real-time
//! stream drops any packet over 9000 bytes, and the ETW path skipped packets
//! in ways it never reported. A merged datagram arrives here whole (an IP
//! packet is at most 65,535 bytes), and the KCP parser reads every segment in
//! it. Needs administrator rights, which the app has.
//!
//! One socket per IPv4 address of every interface that is up (the one the
//! game uses is not known in advance; a VPN moves it), each read on its own
//! thread with a 64 MB receive buffer: a login's burst is ~4 MB in a second.

use std::mem::MaybeUninit;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tokio::sync::mpsc;
use windows::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses,
    IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{
    AF_INET, RCVALL_ON, SIO_RCVALL, SOCKADDR_IN, SOCKET, WSAIoctl,
};

use crate::capture::dedupe::{DEDUPE_CAPACITY, DEDUPE_WINDOW, Deduper};
use crate::capture::pktmon_backend::wrap_ip_in_ethernet;
use crate::capture::{CaptureBackend, CaptureError, DUPLICATES_DROPPED, PORT_RANGE, Result};

/// Per socket. The game's login sends ~4 MB in a second and the stack copies
/// every packet on the interface here, a download included.
const RECEIVE_BUFFER: usize = 64 * 1024 * 1024;

/// The largest IP packet, with room to spare.
const READ_SIZE: usize = 65_536 + 64;

/// How often a blocked read wakes to see whether the backend is gone.
const READ_TIMEOUT: Duration = Duration::from_millis(250);

/// Software loopback (`IF_TYPE_SOFTWARE_LOOPBACK`): nothing from the game.
const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;

pub struct RawSocketBackend {
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    /// Set on drop; the reader threads end at their next read timeout.
    stop: Arc<AtomicBool>,
    dedupe: Deduper,
}

impl RawSocketBackend {
    pub fn new() -> Result<Self> {
        let addresses = interface_addresses().map_err(|e| CaptureError::Capture {
            has_captured: false,
            error: anyhow::anyhow!("can't list the network interfaces: {e}"),
        })?;
        let (tx, rx) = mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mut opened = 0;
        let mut last_error = None;
        for (name, address) in addresses {
            match open_receive_all(address) {
                Ok(socket) => {
                    tracing::info!(interface = %name, %address, "raw capture socket open");
                    opened += 1;
                    let tx = tx.clone();
                    let stop = stop.clone();
                    std::thread::Builder::new()
                        .name(format!("teyvatme-raw-{address}"))
                        .spawn(move || read_loop(socket, tx, stop))
                        .map_err(|e| CaptureError::Capture {
                            has_captured: false,
                            error: anyhow::anyhow!("can't start a capture thread: {e}"),
                        })?;
                }
                Err(e) => {
                    tracing::warn!(interface = %name, %address, "raw capture socket refused: {e}");
                    last_error = Some(e);
                }
            }
        }
        if opened == 0 {
            return Err(CaptureError::Capture {
                has_captured: false,
                error: anyhow::anyhow!(
                    "no interface accepted a receive-all socket{}",
                    last_error.map(|e| format!(": {e}")).unwrap_or_default()
                ),
            });
        }
        Ok(Self {
            rx,
            stop,
            dedupe: Deduper::new(DEDUPE_CAPACITY, DEDUPE_WINDOW),
        })
    }
}

impl Drop for RawSocketBackend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A raw IPv4 socket bound to `address`, receiving every packet on that
/// interface.
fn open_receive_all(address: Ipv4Addr) -> std::io::Result<Socket> {
    let socket = Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::from(0)))?;
    socket.bind(&SockAddr::from(SocketAddrV4::new(address, 0)))?;
    socket.set_recv_buffer_size(RECEIVE_BUFFER)?;
    socket.set_read_timeout(Some(READ_TIMEOUT))?;
    let on: u32 = RCVALL_ON.0 as u32;
    let mut returned = 0u32;
    // SAFETY: the in buffer is a live u32 of the stated size; no out buffer,
    // no overlapped I/O.
    let result = unsafe {
        use std::os::windows::io::AsRawSocket;
        WSAIoctl(
            SOCKET(socket.as_raw_socket() as usize),
            SIO_RCVALL,
            Some(&on as *const u32 as *const _),
            std::mem::size_of::<u32>() as u32,
            None,
            0,
            &mut returned,
            None,
            None,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(socket)
}

/// Read packets until the backend is dropped or the monitor stops listening.
fn read_loop(socket: Socket, tx: mpsc::UnboundedSender<Vec<u8>>, stop: Arc<AtomicBool>) {
    let mut buffer: Vec<MaybeUninit<u8>> = vec![MaybeUninit::uninit(); READ_SIZE];
    while !stop.load(Ordering::Relaxed) {
        let len = match socket.recv(&mut buffer) {
            Ok(len) => len,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(e) => {
                tracing::warn!("raw capture socket read failed: {e}");
                break;
            }
        };
        // SAFETY: `recv` initialised the first `len` bytes.
        let ip = unsafe { std::slice::from_raw_parts(buffer.as_ptr() as *const u8, len) };
        if !is_game_udp(ip) {
            continue;
        }
        let Some(frame) = wrap_ip_in_ethernet(ip) else {
            continue;
        };
        if tx.send(frame).is_err() {
            break;
        }
    }
}

/// Whether `ip` is a UDP packet to or from a game port: the socket has no
/// filter, so this is what pktmon's port filter did. A merged datagram is one
/// UDP header followed by every segment, so it passes like a single one.
fn is_game_udp(ip: &[u8]) -> bool {
    if ip.len() < 28 || ip[0] >> 4 != 4 || ip[9] != 17 {
        return false;
    }
    let header_len = usize::from(ip[0] & 0x0F) * 4;
    let Some(ports) = ip.get(header_len..header_len + 4) else {
        return false;
    };
    let source = u16::from_be_bytes([ports[0], ports[1]]);
    let destination = u16::from_be_bytes([ports[2], ports[3]]);
    (PORT_RANGE.0..=PORT_RANGE.1).contains(&source)
        || (PORT_RANGE.0..=PORT_RANGE.1).contains(&destination)
}

/// The IPv4 addresses of every interface that is up, but the loopback, with
/// the interface's name.
fn interface_addresses() -> std::io::Result<Vec<(String, Ipv4Addr)>> {
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size = 16 * 1024u32;
    let mut buffer: Vec<u8>;
    loop {
        buffer = vec![0; size as usize];
        // SAFETY: the buffer is `size` bytes, as told; the API writes a linked
        // list inside it and `size` back.
        let status = unsafe {
            GetAdaptersAddresses(
                AF_INET.0 as u32,
                flags,
                None,
                Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            )
        };
        match status {
            0 => break,
            // ERROR_BUFFER_OVERFLOW: `size` now says how much is needed.
            111 => continue,
            other => return Err(std::io::Error::from_raw_os_error(other as i32)),
        }
    }

    let mut found = Vec::new();
    let mut adapter = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    // SAFETY: the list was written by GetAdaptersAddresses into `buffer`,
    // which outlives this walk; every pointer followed is one it set.
    unsafe {
        while !adapter.is_null() {
            let a = &*adapter;
            if a.OperStatus == IfOperStatusUp && a.IfType != IF_TYPE_SOFTWARE_LOOPBACK {
                let name = a.FriendlyName.to_string().unwrap_or_default();
                let mut unicast = a.FirstUnicastAddress;
                while !unicast.is_null() {
                    let u = &*unicast;
                    let sockaddr = u.Address.lpSockaddr;
                    if !sockaddr.is_null() && (*sockaddr).sa_family == AF_INET {
                        let v4 = &*(sockaddr as *const SOCKADDR_IN);
                        let ip = Ipv4Addr::from(u32::from_be(v4.sin_addr.S_un.S_addr));
                        if !ip.is_loopback() && !ip.is_unspecified() {
                            found.push((name.clone(), ip));
                        }
                    }
                    unicast = u.Next;
                }
            }
            adapter = a.Next;
        }
    }
    Ok(found)
}

#[async_trait]
impl CaptureBackend for RawSocketBackend {
    async fn next_packet(&mut self) -> Result<Vec<u8>> {
        loop {
            let Some(frame) = self.rx.recv().await else {
                return Err(CaptureError::CaptureClosed);
            };
            // One copy per packet is the norm here; a packet sent and received
            // on one host, or seen by two sockets, is the exception this keeps
            // from reaching the decoder twice.
            if self.dedupe.is_duplicate(&frame, Instant::now()) {
                DUPLICATES_DROPPED.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            return Ok(frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn udp(source: u16, destination: u16, proto: u8) -> Vec<u8> {
        let mut ip = vec![0u8; 20];
        ip[0] = 0x45;
        ip[9] = proto;
        ip.extend_from_slice(&source.to_be_bytes());
        ip.extend_from_slice(&destination.to_be_bytes());
        ip.extend_from_slice(&[0, 8, 0, 0]);
        ip
    }

    #[test]
    fn only_game_port_udp_passes() {
        assert!(is_game_udp(&udp(PORT_RANGE.0, 55584, 17)));
        assert!(is_game_udp(&udp(55584, PORT_RANGE.1, 17)));
        assert!(!is_game_udp(&udp(53, 55584, 17)));
        assert!(!is_game_udp(&udp(PORT_RANGE.0, 55584, 6)));
        assert!(!is_game_udp(&[0x60; 40]));
        assert!(!is_game_udp(&[]));
    }

    #[test]
    fn the_machine_has_an_interface_to_capture_on() {
        let addresses = interface_addresses().unwrap();
        assert!(!addresses.is_empty(), "no interface that is up");
        for (name, ip) in &addresses {
            assert!(!name.is_empty());
            assert!(!ip.is_loopback());
        }
    }
}
