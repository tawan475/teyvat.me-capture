// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Native side of Teyvat.me's Android app.
//!
//! The app's `VpnService` routes only the game's package into a tun device
//! and hands its file descriptor here. Every packet the game sends is read
//! from the tun, every reply is written back to it, and in between a
//! userspace stack (`ipstack`) turns them into TCP and UDP flows that go out
//! through ordinary sockets. The app itself is not in the VPN's allowed list,
//! so those sockets bypass the tunnel without `protect()` and always use the
//! phone's current default network.
//!
//! The game's KCP traffic (UDP 22101–22102, both directions) is copied aside
//! as it passes, unchanged, to the [`decoder`], which builds the account the
//! app exports, and, when the app records a capture for a bug report, into a
//! pcap file (`LINKTYPE_RAW`) that `teyvatme --replay-export` reads. Nothing
//! is altered on the way through.
//!
//! Two things sit between the tun and the stack:
//! - **The TCP gate.** ipstack answers a SYN with a SYN-ACK at once, so a
//!   connection it saw could only be refused after the app believes it is
//!   connected — which looks like a reset after connecting and defeats
//!   Happy Eyeballs when one address family has no path. The device holds
//!   each new SYN back, connects upstream first, then lets the SYN in (and
//!   hands the socket to [`forward_tcp`]) or answers the app with a RST
//!   itself, so the app sees the upstream's real outcome and timing.
//! - **The virtual resolver.** The VPN names [`VIRTUAL_DNS`] as the game's
//!   DNS server; flows to it go to the underlying network's resolvers of the
//!   moment, so a Wi-Fi ↔ mobile switch needs no new tunnel.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll, ready};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use etherparse::{IpNumber, IpSlice, PacketBuilder, TcpSlice};
use ipstack::{
    IpStack, IpStackConfig, IpStackStream, IpStackTcpStream, IpStackUdpStream, TcpConfig,
};
use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jstring};
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{mpsc, oneshot, watch};

mod decoder;
mod link;
mod logging;
mod wish;
mod wish_sync;

/// The game server's KCP ports (Teyvat.me's `capture::PORT_RANGE`).
const GAME_PORTS: (u16, u16) = (22101, 22102);
/// `LINKTYPE_RAW`: bare IPv4/IPv6 packets, what the tee writes.
const LINKTYPE_RAW: u32 = 101;
const MTU: u16 = 1500;
/// The DNS server the VPN gives the game (`GameVpnService.VIRTUAL_DNS`).
/// Nothing listens there: its flows go to the underlying resolvers.
const VIRTUAL_DNS: Ipv4Addr = Ipv4Addr::new(10, 111, 0, 53);
/// Used when the underlying network names no resolver we can parse.
const FALLBACK_DNS: Ipv4Addr = Ipv4Addr::new(1, 1, 1, 1);
/// How long a held SYN may wait for its upstream connect.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a let-in SYN's flow stays known to the gate, so a retransmitted
/// SYN goes to the stack instead of opening a second upstream connection.
const LET_IN_FOR: Duration = Duration::from_secs(30);
/// How long an established TCP flow may sit without data: 2 h 4 min, the
/// least RFC 5382 lets a NAT keep one, so the tunnel never cuts sooner.
const TCP_IDLE_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60 + 4 * 60);

#[derive(Default)]
struct Stats {
    packets_in: AtomicU64,
    packets_out: AtomicU64,
    game_packets: AtomicU64,
    game_bytes: AtomicU64,
    tcp_flows: AtomicU64,
    tcp_refused: AtomicU64,
    udp_flows: AtomicU64,
    udp_rebinds: AtomicU64,
    flow_errors: AtomicU64,
    network_changes: AtomicU64,
}

impl Stats {
    fn line(&self) -> String {
        format!(
            "in={} out={} game_packets={} game_bytes={} tcp_flows={} tcp_refused={} udp_flows={} udp_rebinds={} \
             flow_errors={} network_changes={}",
            self.packets_in.load(Ordering::Relaxed),
            self.packets_out.load(Ordering::Relaxed),
            self.game_packets.load(Ordering::Relaxed),
            self.game_bytes.load(Ordering::Relaxed),
            self.tcp_flows.load(Ordering::Relaxed),
            self.tcp_refused.load(Ordering::Relaxed),
            self.udp_flows.load(Ordering::Relaxed),
            self.udp_rebinds.load(Ordering::Relaxed),
            self.flow_errors.load(Ordering::Relaxed),
            self.network_changes.load(Ordering::Relaxed),
        )
    }
}

/// The underlying network as the app last reported it.
struct Network {
    resolvers: RwLock<Vec<IpAddr>>,
    /// Bumped when the default network changes: UDP flows open a new
    /// upstream socket on it (a connected socket keeps the old source).
    changes: watch::Sender<u64>,
}

impl Network {
    fn new(resolvers: &str) -> Self {
        Self {
            resolvers: RwLock::new(parse_resolvers(resolvers)),
            changes: watch::Sender::new(0),
        }
    }

    /// Where a flow to `dst` really goes: the current resolver for the
    /// virtual one, else `dst` itself.
    fn upstream(&self, dst: SocketAddr) -> SocketAddr {
        if dst.ip() != IpAddr::V4(VIRTUAL_DNS) {
            return dst;
        }
        let resolver = self
            .resolvers
            .read()
            .unwrap()
            .first()
            .copied()
            .unwrap_or(IpAddr::V4(FALLBACK_DNS));
        SocketAddr::new(resolver, dst.port())
    }
}

/// `"1.2.3.4,2001:db8::1"` → addresses; scoped (`fe80::1%wlan0`) and
/// malformed entries are skipped.
fn parse_resolvers(list: &str) -> Vec<IpAddr> {
    list.split(',')
        .filter_map(|s| s.trim().parse().ok())
        // The tunnel's own resolver would send DNS round in a circle.
        .filter(|ip| *ip != IpAddr::V4(VIRTUAL_DNS))
        .collect()
}

type FlowKey = (SocketAddr, SocketAddr);

enum Slot {
    Connecting,
    LetIn(Instant),
}

/// The TCP gate's state (see the crate docs).
#[derive(Default)]
struct Gate {
    syns: HashMap<FlowKey, Slot>,
    /// Upstream sockets connected for a let-in SYN, until `forward_tcp`
    /// takes them.
    sockets: HashMap<FlowKey, (TcpStream, Instant)>,
}

impl Gate {
    fn prune(&mut self, now: Instant) {
        self.syns.retain(|_, slot| match slot {
            Slot::Connecting => true,
            Slot::LetIn(at) => now.duration_since(*at) < LET_IN_FOR,
        });
        self.sockets
            .retain(|_, (_, at)| now.duration_since(*at) < LET_IN_FOR);
    }
}

struct Shared {
    stats: Stats,
    network: Network,
    gate: Mutex<Gate>,
}

struct Running {
    runtime: tokio::runtime::Runtime,
    stop: oneshot::Sender<()>,
    shared: Arc<Shared>,
    /// The tun, for [`stop`] to see whether a task still holds it.
    tun: Tun,
}

/// The tun descriptor, shared by the device, the gate's resets and
/// [`Running`]; closed when the last holder drops it. Android ends the VPN
/// only once the tun is closed, so [`stop`] closes it under any task the
/// shutdown left behind ([`release_tun`]).
type Tun = Arc<OwnedFd>;

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

/// UDP source/destination ports of a bare IP packet, or `None` when it is
/// not UDP. IPv6 extension headers are not walked (the game sends none).
fn udp_ports(packet: &[u8]) -> Option<(u16, u16)> {
    let (proto, offset) = match packet.first()? >> 4 {
        4 => (*packet.get(9)?, usize::from(packet[0] & 0x0f) * 4),
        6 => (*packet.get(6)?, 40),
        _ => return None,
    };
    if proto != 17 {
        return None;
    }
    let ports = packet.get(offset..offset + 4)?;
    Some((
        u16::from_be_bytes([ports[0], ports[1]]),
        u16::from_be_bytes([ports[2], ports[3]]),
    ))
}

fn is_game_port(port: u16) -> bool {
    (GAME_PORTS.0..=GAME_PORTS.1).contains(&port)
}

fn is_game_packet(packet: &[u8]) -> bool {
    udp_ports(packet).is_some_and(|(src, dst)| is_game_port(src) || is_game_port(dst))
}

/// A connection request from the app: a TCP SYN without ACK.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Syn {
    src: SocketAddr,
    dst: SocketAddr,
    seq: u32,
}

impl Syn {
    fn parse(packet: &[u8]) -> Option<Syn> {
        let ip = IpSlice::from_slice(packet).ok()?;
        let payload = ip.payload();
        if payload.ip_number != IpNumber::TCP || payload.fragmented {
            return None;
        }
        let tcp = TcpSlice::from_slice(payload.payload).ok()?;
        if !tcp.syn() || tcp.ack() || tcp.rst() {
            return None;
        }
        Some(Syn {
            src: SocketAddr::new(ip.source_addr(), tcp.source_port()),
            dst: SocketAddr::new(ip.destination_addr(), tcp.destination_port()),
            seq: tcp.sequence_number(),
        })
    }

    fn key(&self) -> FlowKey {
        (self.src, self.dst)
    }

    /// The RST|ACK a host sends for a refused connection (RFC 9293 3.10.7.1).
    fn reset(&self) -> Option<Vec<u8>> {
        let builder = match (self.dst.ip(), self.src.ip()) {
            (IpAddr::V4(from), IpAddr::V4(to)) => {
                PacketBuilder::ipv4(from.octets(), to.octets(), 64)
            }
            (IpAddr::V6(from), IpAddr::V6(to)) => {
                PacketBuilder::ipv6(from.octets(), to.octets(), 64)
            }
            _ => return None,
        };
        let builder = builder
            .tcp(self.dst.port(), self.src.port(), 0, 0)
            .rst()
            .ack(self.seq.wrapping_add(1));
        let mut out = Vec::with_capacity(builder.size(0));
        builder.write(&mut out, &[]).ok()?;
        Some(out)
    }
}

/// The tun file descriptor as the async device `ipstack` reads and writes,
/// one packet per call, copying game packets to the tee on the way and
/// holding new SYNs at the gate.
struct TunDevice {
    fd: Arc<AsyncFd<Tun>>,
    /// The recording's writer, when the app records the capture.
    tee: Option<mpsc::UnboundedSender<(SystemTime, Vec<u8>)>>,
    shared: Arc<Shared>,
    /// SYNs whose upstream connected, handed to the stack before the tun.
    let_in: mpsc::UnboundedReceiver<Vec<u8>>,
    let_in_tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl TunDevice {
    fn new(
        fd: Tun,
        tee: Option<mpsc::UnboundedSender<(SystemTime, Vec<u8>)>>,
        shared: Arc<Shared>,
    ) -> io::Result<Self> {
        let raw = fd.as_raw_fd();
        // SAFETY: `raw` is the open tun descriptor we own.
        let flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the descriptor stays open while the `AsyncFd` lives: it is
        // closed when the last `Tun` holder drops it (the `AsyncFd` is one).
        // `release_tun` swaps the file only after the runtime shut down.
        let fd = unsafe { AsyncFd::register(fd) }.map_err(|e| io::Error::other(e.to_string()))?;
        let (let_in_tx, let_in) = mpsc::unbounded_channel();
        Ok(Self {
            fd: Arc::new(fd),
            tee,
            shared,
            let_in,
            let_in_tx,
        })
    }

    fn copy_if_game(&self, packet: &[u8]) {
        if is_game_packet(packet) {
            self.shared
                .stats
                .game_packets
                .fetch_add(1, Ordering::Relaxed);
            self.shared
                .stats
                .game_bytes
                .fetch_add(packet.len() as u64, Ordering::Relaxed);
            let at = SystemTime::now();
            decoder::get().packet(at, packet.to_vec());
            if let Some(tee) = &self.tee {
                let _ = tee.send((at, packet.to_vec()));
            }
        }
    }

    /// True when the gate keeps `packet` from the stack: a new SYN (its
    /// upstream connect starts now) or a retransmitted one still waiting.
    fn hold_at_gate(&self, packet: &[u8]) -> bool {
        let Some(syn) = Syn::parse(packet) else {
            return false;
        };
        let mut gate = self.shared.gate.lock().unwrap();
        gate.prune(Instant::now());
        match gate.syns.get(&syn.key()) {
            Some(Slot::Connecting) => true,
            Some(Slot::LetIn(_)) => false,
            None => {
                gate.syns.insert(syn.key(), Slot::Connecting);
                tokio::spawn(open_upstream(
                    self.shared.clone(),
                    self.fd.clone(),
                    self.let_in_tx.clone(),
                    syn,
                    packet.to_vec(),
                ));
                true
            }
        }
    }
}

fn read_fd(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    // SAFETY: `buf` is valid for `buf.len()` bytes.
    let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

fn write_fd(fd: RawFd, buf: &[u8]) -> io::Result<usize> {
    // SAFETY: `buf` is valid for `buf.len()` bytes.
    let n = unsafe { libc::write(fd, buf.as_ptr().cast(), buf.len()) };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

/// Writes one packet to the tun from outside the stack (the gate's RSTs);
/// a tun write is one whole packet, so it never interleaves with the stack's.
async fn write_packet(fd: &AsyncFd<Tun>, packet: &[u8]) -> io::Result<()> {
    loop {
        let mut guard = fd.writable().await?;
        match guard.try_io(|fd| write_fd(fd.as_raw_fd(), packet)) {
            Ok(result) => return result.map(drop),
            Err(_would_block) => continue,
        }
    }
}

/// The gate's upstream connect for one held SYN (see the crate docs).
async fn open_upstream(
    shared: Arc<Shared>,
    fd: Arc<AsyncFd<Tun>>,
    let_in: mpsc::UnboundedSender<Vec<u8>>,
    syn: Syn,
    packet: Vec<u8>,
) {
    let target = shared.network.upstream(syn.dst);
    let result = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(target)).await;
    match result {
        Ok(Ok(socket)) => {
            let now = Instant::now();
            let mut gate = shared.gate.lock().unwrap();
            gate.syns.insert(syn.key(), Slot::LetIn(now));
            gate.sockets.insert(syn.key(), (socket, now));
            drop(gate);
            let _ = let_in.send(packet);
        }
        failed => {
            shared.gate.lock().unwrap().syns.remove(&syn.key());
            shared.stats.tcp_refused.fetch_add(1, Ordering::Relaxed);
            match failed {
                Ok(Err(e)) => log::debug!("tcp connect {target}: {e}"),
                _ => log::debug!("tcp connect {target}: timed out"),
            }
            if let Some(reset) = syn.reset()
                && let Err(e) = write_packet(&fd, &reset).await
            {
                log::debug!("tcp reset for {}: {e}", syn.src);
            }
        }
    }
}

impl AsyncRead for TunDevice {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        loop {
            if let Poll::Ready(Some(syn)) = this.let_in.poll_recv(cx) {
                if syn.len() <= buf.remaining() {
                    buf.put_slice(&syn);
                    return Poll::Ready(Ok(()));
                }
                continue;
            }
            let mut guard = ready!(this.fd.poll_read_ready(cx))?;
            let unfilled = buf.initialize_unfilled();
            match guard.try_io(|fd| read_fd(fd.as_raw_fd(), unfilled)) {
                Ok(Ok(n)) => {
                    this.shared
                        .stats
                        .packets_out
                        .fetch_add(1, Ordering::Relaxed);
                    let packet = &buf.initialize_unfilled()[..n];
                    this.copy_if_game(packet);
                    if this.hold_at_gate(packet) {
                        continue;
                    }
                    buf.advance(n);
                    return Poll::Ready(Ok(()));
                }
                Ok(Err(e)) => return Poll::Ready(Err(e)),
                Err(_would_block) => continue,
            }
        }
    }
}

impl AsyncWrite for TunDevice {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        packet: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        loop {
            let mut guard = ready!(this.fd.poll_write_ready(cx))?;
            match guard.try_io(|fd| write_fd(fd.as_raw_fd(), packet)) {
                Ok(Ok(n)) => {
                    this.shared.stats.packets_in.fetch_add(1, Ordering::Relaxed);
                    this.copy_if_game(packet);
                    return Poll::Ready(Ok(n));
                }
                Ok(Err(e)) => return Poll::Ready(Err(e)),
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Writes the tee's packets as a classic pcap file, flushed every second so
/// a capture pulled while the VPN still runs is readable up to then.
async fn write_pcap(
    path: String,
    mut packets: mpsc::UnboundedReceiver<(SystemTime, Vec<u8>)>,
) -> io::Result<()> {
    let mut out = BufWriter::new(File::create(&path)?);
    out.write_all(&0xa1b2_c3d4u32.to_le_bytes())?;
    out.write_all(&2u16.to_le_bytes())?;
    out.write_all(&4u16.to_le_bytes())?;
    out.write_all(&0i32.to_le_bytes())?;
    out.write_all(&0u32.to_le_bytes())?;
    out.write_all(&u32::from(MTU).max(65535).to_le_bytes())?;
    out.write_all(&LINKTYPE_RAW.to_le_bytes())?;
    out.flush()?;
    let mut flush = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            packet = packets.recv() => {
                let Some((at, data)) = packet else { break };
                let since = at.duration_since(UNIX_EPOCH).unwrap_or_default();
                out.write_all(&(since.as_secs() as u32).to_le_bytes())?;
                out.write_all(&since.subsec_micros().to_le_bytes())?;
                out.write_all(&(data.len() as u32).to_le_bytes())?;
                out.write_all(&(data.len() as u32).to_le_bytes())?;
                out.write_all(&data)?;
            }
            _ = flush.tick() => out.flush()?,
        }
    }
    out.flush()
}

async fn forward_tcp(mut flow: IpStackTcpStream, shared: Arc<Shared>) {
    let peer = flow.peer_addr();
    let held = shared
        .gate
        .lock()
        .unwrap()
        .sockets
        .remove(&(flow.local_addr(), peer));
    let mut upstream = match held {
        Some((socket, _)) => socket,
        // A SYN the gate had let in once already (the app reused the port
        // within `LET_IN_FOR`): connect now, as the gate would have.
        None => {
            let target = shared.network.upstream(peer);
            match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(target)).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    shared.stats.flow_errors.fetch_add(1, Ordering::Relaxed);
                    log::debug!("tcp connect {target}: {e}");
                    return;
                }
                Err(_) => {
                    shared.stats.flow_errors.fetch_add(1, Ordering::Relaxed);
                    log::debug!("tcp connect {target}: timed out");
                    return;
                }
            }
        }
    };
    let _ = upstream.set_nodelay(true);
    if let Err(e) = tokio::io::copy_bidirectional(&mut flow, &mut upstream).await {
        log::debug!("tcp {peer}: {e}");
    }
    let _ = upstream.shutdown().await;
    let _ = flow.shutdown().await;
}

async fn udp_upstream(target: SocketAddr) -> io::Result<UdpSocket> {
    let bind: SocketAddr = if target.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    }
    .parse()
    .unwrap();
    let socket = UdpSocket::bind(bind).await?;
    socket.connect(target).await?;
    Ok(socket)
}

async fn forward_udp(mut flow: IpStackUdpStream, shared: Arc<Shared>) {
    let peer = flow.peer_addr();
    let mut changes = shared.network.changes.subscribe();
    let mut from_game = vec![0u8; usize::from(MTU)];
    let mut from_server = vec![0u8; 65535];
    // A new upstream socket after each default-network change: the old one
    // stays bound to the source address it connected from. The game's side
    // of the flow is unchanged, so its KCP session sees one path that
    // continues from a new address.
    'rebind: loop {
        let target = shared.network.upstream(peer);
        let socket = match udp_upstream(target).await {
            Ok(s) => s,
            Err(e) => {
                shared.stats.flow_errors.fetch_add(1, Ordering::Relaxed);
                log::debug!("udp upstream {target}: {e}");
                return;
            }
        };
        changes.mark_unchanged();
        loop {
            tokio::select! {
                read = flow.read(&mut from_game) => match read {
                    // ipstack yields 0 at the end of the flow (an empty
                    // datagram reads the same; the game sends none).
                    Ok(0) | Err(_) => break 'rebind,
                    Ok(n) => if let Err(e) = socket.send(&from_game[..n]).await {
                        log::debug!("udp {target}: {e}");
                        break 'rebind;
                    },
                },
                recv = socket.recv(&mut from_server) => match recv {
                    Ok(n) => if flow.write_all(&from_server[..n]).await.is_err() { break 'rebind },
                    Err(e) => {
                        log::debug!("udp {target}: {e}");
                        break 'rebind;
                    }
                },
                Ok(()) = changes.changed() => {
                    shared.stats.udp_rebinds.fetch_add(1, Ordering::Relaxed);
                    if is_game_port(peer.port()) {
                        log::info!("game flow {peer}: network changed, new upstream socket");
                    }
                    continue 'rebind;
                }
            }
        }
    }
}

async fn run(
    fd: Tun,
    pcap_path: Option<String>,
    shared: Arc<Shared>,
    stop: oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let (tee, writer) = match pcap_path {
        Some(path) => {
            let (tee, packets) = mpsc::unbounded_channel();
            (Some(tee), Some(tokio::spawn(write_pcap(path, packets))))
        }
        None => (None, None),
    };
    let device = TunDevice::new(fd, tee, shared.clone())?;

    let mut config = IpStackConfig::default();
    config.mtu(MTU)?;
    // The game keeps its KCP flow busy; a minute of silence means it is gone.
    config.udp_timeout(Duration::from_secs(60));
    // ipstack drops a TCP flow after 60 s without data, which cut idle
    // HTTPS keep-alive connections (seen on the game's CDN flows).
    let mut tcp = TcpConfig::default();
    tcp.timeout = TCP_IDLE_TIMEOUT;
    config.with_tcp_config(tcp);
    let mut stack = IpStack::new(config, device);

    let mut report = tokio::time::interval(Duration::from_secs(10));
    tokio::pin!(stop);
    loop {
        tokio::select! {
            _ = &mut stop => break,
            _ = report.tick() => log::info!("{}", shared.stats.line()),
            flow = stack.accept() => match flow? {
                IpStackStream::Tcp(flow) => {
                    shared.stats.tcp_flows.fetch_add(1, Ordering::Relaxed);
                    tokio::spawn(forward_tcp(flow, shared.clone()));
                }
                IpStackStream::Udp(flow) => {
                    shared.stats.udp_flows.fetch_add(1, Ordering::Relaxed);
                    tokio::spawn(forward_udp(flow, shared.clone()));
                }
                IpStackStream::UnknownTransport(_) | IpStackStream::UnknownNetwork(_) => {}
            },
        }
    }
    // Dropping the stack closes the tun (the VPN ends); the tee's sender goes
    // with it, so the writer drains and flushes.
    drop(stack);
    if let Some(writer) = writer {
        let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
    }
    log::info!("stopped: {}", shared.stats.line());
    Ok(())
}

/// Starts the forwarder on `fd`; on an error the tun is closed (dropped), so
/// the VPN the app just established ends with it.
fn start(fd: OwnedFd, pcap_path: Option<String>, resolvers: &str) -> anyhow::Result<()> {
    let mut running = RUNNING.lock().unwrap();
    if running.is_some() {
        anyhow::bail!("already running");
    }
    let tun: Tun = Arc::new(fd);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("teyvatme-vpn")
        .build()?;
    let shared = Arc::new(Shared {
        stats: Stats::default(),
        network: Network::new(resolvers),
        gate: Mutex::default(),
    });
    decoder::get().capture_started();
    let (stop, stopped) = oneshot::channel();
    let task_shared = shared.clone();
    let task_tun = tun.clone();
    runtime.spawn(async move {
        if let Err(e) = run(task_tun, pcap_path, task_shared, stopped).await {
            log::error!("forwarder ended: {e:#}");
        }
    });
    *running = Some(Running {
        runtime,
        stop,
        shared,
        tun,
    });
    Ok(())
}

fn stop() {
    let Some(running) = RUNNING.lock().unwrap().take() else {
        return;
    };
    let _ = running.stop.send(());
    running.runtime.shutdown_timeout(Duration::from_secs(3));
    // Normally the shutdown dropped every task and `running.tun` is the last
    // holder, so dropping it closes the tun. A task the shutdown could not
    // drop (a worker stuck in its poll) still holds it, and Android kept the
    // VPN up with nothing forwarding until the user disconnected it by hand.
    let holders = Arc::strong_count(&running.tun) - 1;
    if holders > 0 {
        log::warn!("stop: {holders} task(s) still hold the tun; closing it under them");
        release_tun(&running.tun);
    }
}

/// Closes the tun under tasks that still hold it, which ends the VPN:
/// `/dev/null` takes the descriptor's number over, so the number stays valid
/// (reads end, writes vanish) until the last holder drops it, and no task
/// can reach a file opened later under that number. Only for tasks the
/// runtime has given up on: their `AsyncFd` then sees a different file
/// under its number, which tokio's contract rules out for a live reactor.
fn release_tun(tun: &OwnedFd) {
    // SAFETY: plain syscalls on a path literal and on descriptors we own;
    // `dup2` swaps the file under `tun`'s number atomically (and clears
    // close-on-exec there, set again after).
    unsafe {
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
        if null < 0 {
            log::error!("stop: /dev/null: {}", io::Error::last_os_error());
            return;
        }
        let fd = tun.as_raw_fd();
        if libc::dup2(null, fd) < 0 {
            log::error!("stop: closing the tun: {}", io::Error::last_os_error());
        } else {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        libc::close(null);
    }
}

fn network_changed(resolvers: &str, new_network: bool) {
    let running = RUNNING.lock().unwrap();
    let Some(running) = running.as_ref() else {
        return;
    };
    let network = &running.shared.network;
    *network.resolvers.write().unwrap() = parse_resolvers(resolvers);
    if new_network {
        running
            .shared
            .stats
            .network_changes
            .fetch_add(1, Ordering::Relaxed);
        network.changes.send_modify(|n| *n += 1);
    }
}

fn java_string(env: &mut JNIEnv, s: &JString) -> Option<String> {
    match env.get_string(s) {
        Ok(s) => Some(s.into()),
        Err(e) => {
            log::error!("bad string from Java: {e}");
            None
        }
    }
}

/// `NativeBridge.init(logDir)`: logging to logcat and `logDir/latest.log`,
/// and the decoder (it loads the game data in the background). Once per
/// process; later calls do nothing.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_init(
    mut env: JNIEnv,
    _class: JClass,
    log_dir: JString,
) {
    if let Some(dir) = java_string(&mut env, &log_dir) {
        logging::init(std::path::Path::new(&dir));
    }
    // The wish sync's way to the app's tracker sign-in, found here: a thread
    // of our own cannot find app classes.
    match (
        env.get_java_vm(),
        env.find_class("me/teyvat/app/WishTrackerBridge")
            .and_then(|class| env.new_global_ref(class)),
    ) {
        (Ok(vm), Ok(class)) => wish_sync::keep_bridge(vm, class),
        (vm, class) => {
            let _ = env.exception_clear();
            log::error!(
                "wish sync: no bridge to the app: {:?} {:?}",
                vm.err(),
                class.err()
            );
        }
    }
    decoder::get();
}

/// `NativeBridge.trackerExport(manual, live)`: the tracker's snapshot as
/// `"<ticket>\n<GOOD JSON>"`, "" when nothing is due (live mode: batches
/// carry the changes), or null before anything is captured.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_trackerExport(
    env: JNIEnv,
    _class: JClass,
    manual: jboolean,
    live: jboolean,
) -> jstring {
    let export = match decoder::get().tracker_export(manual != 0, live != 0) {
        Ok(Some(Some((ticket, json)))) => Some(format!("{ticket}\n{json}")),
        Ok(Some(None)) => Some(String::new()),
        Ok(None) => None,
        Err(e) => {
            log::error!("tracker export: {e:#}");
            None
        }
    };
    java_result(&env, export)
}

/// `NativeBridge.liveNextTick(live)`: when live sync next has something to
/// do (Unix ms), or -1.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_liveNextTick(
    _env: JNIEnv,
    _class: JClass,
    live: jboolean,
) -> jni::sys::jlong {
    decoder::get()
        .live_next_tick(live != 0)
        .and_then(|at| i64::try_from(at).ok())
        .unwrap_or(-1)
}

/// `NativeBridge.liveTick(live)`: `"S<ticket>\n<GOOD JSON>"` (a snapshot),
/// `"B\n<delta JSON>"` (a batch for `/irminsul/changes`), or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_liveTick(
    env: JNIEnv,
    _class: JClass,
    live: jboolean,
) -> jstring {
    let action = decoder::get().live_tick(live != 0).unwrap_or_else(|e| {
        log::error!("live sync: {e:#}");
        None
    });
    java_result(&env, action)
}

/// `NativeBridge.snapshotAnswered(ticket, body)`: the snapshot upload
/// finished; `body` is the tracker's answer, or null when it failed.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_snapshotAnswered(
    mut env: JNIEnv,
    _class: JClass,
    ticket: jni::sys::jlong,
    body: JString,
) {
    let body = (!body.is_null())
        .then(|| java_string(&mut env, &body))
        .flatten();
    if let Ok(ticket) = u64::try_from(ticket)
        && ticket > 0
    {
        decoder::get().snapshot_answered(ticket, body.as_deref());
    }
}

/// `NativeBridge.batchAnswered(status, body)`: the batch request finished
/// (status 0: no answer, -1: signed out).
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_batchAnswered(
    mut env: JNIEnv,
    _class: JClass,
    status: jint,
    body: JString,
) {
    let body = java_string(&mut env, &body).unwrap_or_default();
    decoder::get().batch_answered(status, &body);
}

/// `NativeBridge.setWishSync(on)`: whether a captured wish key is synced to
/// the tracker (signed in, uploads on, wishes on).
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_setWishSync(
    _env: JNIEnv,
    _class: JClass,
    on: jboolean,
) {
    decoder::get().wishes.sync.set_enabled(on != 0);
}

/// `NativeBridge.start(fd, pcapPath, resolvers)`: 0 on success, -1 on
/// failure (logged). `resolvers` is a comma-separated address list;
/// `pcapPath` is empty unless the app records the capture.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_start(
    mut env: JNIEnv,
    _class: JClass,
    fd: jint,
    pcap_path: JString,
    resolvers: JString,
) -> jint {
    // SAFETY: the app hands over the tun descriptor with `detachFd()`, so it
    // is ours to own and close — owned first, so every failure below closes
    // it (and the VPN ends) instead of leaking it.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let (Some(path), Some(resolvers)) = (
        java_string(&mut env, &pcap_path),
        java_string(&mut env, &resolvers),
    ) else {
        return -1;
    };
    let recording = Some(path).filter(|path| !path.is_empty());
    match start(fd, recording.clone(), &resolvers) {
        Ok(()) => {
            match recording {
                Some(path) => log::info!("forwarder started, recording game packets to {path}"),
                None => log::info!("forwarder started"),
            }
            0
        }
        Err(e) => {
            log::error!("start: {e:#}");
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_stop(_env: JNIEnv, _class: JClass) {
    stop();
}

/// `NativeBridge.networkChanged(resolvers, newNetwork)`: the underlying
/// network's resolvers now, and whether the default network itself changed.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_networkChanged(
    mut env: JNIEnv,
    _class: JClass,
    resolvers: JString,
    new_network: jboolean,
) {
    if let Some(resolvers) = java_string(&mut env, &resolvers) {
        network_changed(&resolvers, new_network != 0);
    }
}

/// `NativeBridge.stats()`: one line of counters, or "" when not running.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_stats(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let line = RUNNING
        .lock()
        .unwrap()
        .as_ref()
        .map(|r| r.shared.stats.line())
        .unwrap_or_default();
    env.new_string(line)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn java_result(env: &JNIEnv, value: Option<String>) -> jstring {
    match value {
        Some(value) => env
            .new_string(value)
            .map(|s| s.into_raw())
            .unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
    }
}

/// `NativeBridge.summary()`: the decoder's [`decoder::Summary`] as JSON.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_summary(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    java_result(&env, Some(decoder::get().summary_json()))
}

/// `NativeBridge.exportGood(settings)`: the GOOD JSON with the export
/// settings (JSON, the desktop's `ExportSettings`; "" for the defaults), or
/// null when nothing is captured or the settings do not parse (logged).
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_exportGood(
    mut env: JNIEnv,
    _class: JClass,
    settings: JString,
) -> jstring {
    let Some(settings) = java_string(&mut env, &settings) else {
        return std::ptr::null_mut();
    };
    let json = decoder::get().export(&settings).unwrap_or_else(|e| {
        log::error!("export: {e:#}");
        None
    });
    java_result(&env, json)
}

/// `NativeBridge.achievements()`: `{"gi_achievements": [...]}`, or null
/// before achievements were captured.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_achievements(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    java_result(&env, decoder::get().achievements())
}

/// `NativeBridge.note(kind, text)`: an event from the app for the activity
/// list; `kind` is `ok`, `error` or `info`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_note(
    mut env: JNIEnv,
    _class: JClass,
    kind: JString,
    text: JString,
) {
    let (Some(kind), Some(text)) = (java_string(&mut env, &kind), java_string(&mut env, &text))
    else {
        return;
    };
    let kind = match kind.as_str() {
        "ok" => "ok",
        "error" => "error",
        _ => "info",
    };
    decoder::get().note(kind, &text);
}

/// `NativeBridge.wishUrl()`: the game's wish history link for this login's
/// key, or null without one. A password while it lasts: never logged.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_wishUrl(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    java_result(&env, decoder::get().wishes.url())
}

/// `NativeBridge.startWishHistory(appVersion)`: start reading every banner
/// into a UIGF file. 0 started; 1 no key; 2 the key's UID is not known yet;
/// 3 a read is running.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_startWishHistory(
    mut env: JNIEnv,
    _class: JClass,
    app_version: JString,
) -> jint {
    let version = java_string(&mut env, &app_version).unwrap_or_default();
    match decoder::get().wishes.start(&version) {
        Ok(()) => 0,
        Err(wish::StartError::NoKey) => 1,
        Err(wish::StartError::NoUid) => 2,
        Err(wish::StartError::Running) => 3,
    }
}

/// `NativeBridge.cancelWishHistory()`: stop a read in flight.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_cancelWishHistory(
    _env: JNIEnv,
    _class: JClass,
) {
    decoder::get().wishes.cancel();
}

/// `NativeBridge.wishHistory()`: the last read's UIGF v4 JSON, or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_wishHistory(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    java_result(&env, decoder::get().wishes.take())
}

/// `NativeBridge.presenceDue(state, uid, nowMs)`: whether a presence
/// heartbeat for `state` (`stopped`, `running`, `decoding`) and `uid` (≤ 0:
/// unknown) goes now; when true, the app sends it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_presenceDue(
    mut env: JNIEnv,
    _class: JClass,
    state: JString,
    uid: jni::sys::jlong,
    now_ms: jni::sys::jlong,
) -> jboolean {
    let state = java_string(&mut env, &state).unwrap_or_default();
    u8::from(link::presence_due(
        &state,
        uid,
        u64::try_from(now_ms).unwrap_or(0),
    ))
}

/// `NativeBridge.presenceAnswered(status, body, nowMs)`: the heartbeat's
/// answer (status 0: none) as `{"gone": true}` or `{"commands": [...],
/// "alerts"?: {...} | null}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_presenceAnswered(
    mut env: JNIEnv,
    _class: JClass,
    status: jint,
    body: JString,
    now_ms: jni::sys::jlong,
) -> jstring {
    let body = java_string(&mut env, &body).unwrap_or_default();
    let answer = link::presence_answered(status, &body, u64::try_from(now_ms).unwrap_or(0));
    java_result(&env, Some(answer))
}

/// `NativeBridge.linkReset()`: the tracker link is gone or new.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_linkReset(_env: JNIEnv, _class: JClass) {
    link::reset();
}

/// `NativeBridge.syncWishesNow()`: the website's "Sync wishes": this login's
/// wish key syncs now; false without one.
#[unsafe(no_mangle)]
pub extern "system" fn Java_me_teyvat_app_NativeBridge_syncWishesNow(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    u8::from(decoder::get().wishes.hub().sync_now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ipv4_udp(src: u16, dst: u16) -> Vec<u8> {
        let mut p = vec![
            0x45, 0, 0, 28, 0, 0, 0, 0, 64, 17, 0, 0, 10, 0, 0, 2, 8, 8, 8, 8,
        ];
        p.extend_from_slice(&src.to_be_bytes());
        p.extend_from_slice(&dst.to_be_bytes());
        p.extend_from_slice(&[0, 8, 0, 0]);
        p
    }

    #[test]
    fn game_packets_are_udp_on_the_kcp_ports_either_way() {
        assert!(is_game_packet(&ipv4_udp(40000, 22101)));
        assert!(is_game_packet(&ipv4_udp(22102, 40000)));
        assert!(!is_game_packet(&ipv4_udp(40000, 53)));
        let mut tcp = ipv4_udp(40000, 22101);
        tcp[9] = 6;
        assert!(!is_game_packet(&tcp));
        let mut v6 = vec![0x60, 0, 0, 0, 0, 8, 17, 64];
        v6.extend_from_slice(&[0; 32]);
        v6.extend_from_slice(&40000u16.to_be_bytes());
        v6.extend_from_slice(&22101u16.to_be_bytes());
        assert!(is_game_packet(&v6));
        assert!(!is_game_packet(&[0x45, 0]));
        assert!(!is_game_packet(&[]));
    }

    fn syn_packet(src: SocketAddr, dst: SocketAddr, seq: u32, ack: bool) -> Vec<u8> {
        let builder = match (src.ip(), dst.ip()) {
            (IpAddr::V4(s), IpAddr::V4(d)) => PacketBuilder::ipv4(s.octets(), d.octets(), 64),
            (IpAddr::V6(s), IpAddr::V6(d)) => PacketBuilder::ipv6(s.octets(), d.octets(), 64),
            _ => unreachable!(),
        };
        let builder = builder.tcp(src.port(), dst.port(), seq, 65535).syn();
        let builder = if ack { builder.ack(1) } else { builder };
        let mut out = Vec::new();
        builder.write(&mut out, &[]).unwrap();
        out
    }

    #[test]
    fn only_a_bare_syn_is_held() {
        let src: SocketAddr = "10.111.0.2:40000".parse().unwrap();
        let dst: SocketAddr = "203.0.113.7:443".parse().unwrap();
        assert_eq!(
            Syn::parse(&syn_packet(src, dst, 7, false)),
            Some(Syn { src, dst, seq: 7 })
        );
        assert_eq!(Syn::parse(&syn_packet(src, dst, 7, true)), None);
        assert_eq!(Syn::parse(&ipv4_udp(40000, 443)), None);
        let src6: SocketAddr = "[fd00:6972:6d::2]:40000".parse().unwrap();
        let dst6: SocketAddr = "[2001:db8::7]:443".parse().unwrap();
        assert_eq!(
            Syn::parse(&syn_packet(src6, dst6, 9, false)),
            Some(Syn {
                src: src6,
                dst: dst6,
                seq: 9
            })
        );
    }

    #[test]
    fn a_refused_syn_gets_a_valid_rst_ack_back() {
        for (src, dst) in [
            ("10.111.0.2:40000", "203.0.113.7:443"),
            ("[fd00:6972:6d::2]:40000", "[2001:db8::7]:443"),
        ] {
            let syn = Syn {
                src: src.parse().unwrap(),
                dst: dst.parse().unwrap(),
                seq: u32::MAX,
            };
            let reset = syn.reset().unwrap();
            let ip = IpSlice::from_slice(&reset).unwrap();
            assert_eq!(ip.source_addr(), syn.dst.ip());
            assert_eq!(ip.destination_addr(), syn.src.ip());
            let tcp = TcpSlice::from_slice(ip.payload().payload).unwrap();
            assert!(tcp.rst() && tcp.ack() && !tcp.syn());
            assert_eq!(
                (tcp.source_port(), tcp.destination_port()),
                (syn.dst.port(), syn.src.port())
            );
            assert_eq!(tcp.acknowledgment_number(), 0, "seq + 1 wraps");
            let checksum = match (syn.dst.ip(), syn.src.ip()) {
                (IpAddr::V4(s), IpAddr::V4(d)) => {
                    tcp.to_header()
                        .calc_checksum_ipv4_raw(s.octets(), d.octets(), &[])
                }
                (IpAddr::V6(s), IpAddr::V6(d)) => {
                    tcp.to_header()
                        .calc_checksum_ipv6_raw(s.octets(), d.octets(), &[])
                }
                _ => unreachable!(),
            };
            assert_eq!(checksum.unwrap(), tcp.checksum());
        }
    }

    #[test]
    fn the_virtual_resolver_goes_to_the_current_one() {
        let network = Network::new("fe80::1%wlan0, 192.0.2.53,2001:db8::53");
        let dns: SocketAddr = "10.111.0.53:53".parse().unwrap();
        assert_eq!(network.upstream(dns), "192.0.2.53:53".parse().unwrap());
        let other: SocketAddr = "203.0.113.7:53".parse().unwrap();
        assert_eq!(network.upstream(other), other);
        *network.resolvers.write().unwrap() = parse_resolvers("");
        assert_eq!(network.upstream(dns), "1.1.1.1:53".parse().unwrap());
        *network.resolvers.write().unwrap() = parse_resolvers("10.111.0.53");
        assert_eq!(network.upstream(dns), "1.1.1.1:53".parse().unwrap());
    }
}
