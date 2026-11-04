use std::alloc::Layout;
use std::ffi::c_void;
use std::ptr::slice_from_raw_parts;
use std::sync::mpsc;
use std::time::Duration;

use c_api::{
    PacketMonitorDataSourceList, PacketMonitorDataSourceSpecification as Spec,
    PacketMonitorPacketType,
};
use log::{debug, error, info, trace, warn};
use windows::w;

use crate::filter::PktMonFilter;
use crate::util::install_shutdown_hook;
use crate::{CaptureBackend, Packet, PacketPayload};

mod c_api;
mod c_filter;

#[derive(Debug)]
struct MonitorContext {
    sender: mpsc::Sender<Packet>,

    #[cfg(feature = "tokio")]
    notify: Option<std::sync::Weak<tokio::sync::Notify>>,
}

type UserContext = *mut MonitorContext;

#[derive(Debug)]
pub struct RealTimeBackend {
    api: c_api::PacketMonitorApi,

    handle: c_api::PacketMonitorHandle,
    session: c_api::PacketMonitorSession,
    stream: c_api::PacketMonitorRealTimeStream,

    receiver: mpsc::Receiver<Packet>,

    #[allow(dead_code)]
    context: Box<MonitorContext>,

    #[cfg(feature = "tokio")]
    notify: Option<std::sync::Arc<tokio::sync::Notify>>,

    loaded: bool,
}

extern "system" fn event_callback(
    _context: *mut c_void,
    event_info: *const c_api::PacketMonitorStreamEventInfo,
    event_kind: c_api::PacketMonitorStreamEventKind,
) {
    match event_kind {
        c_api::PacketMonitorStreamEventKind::PacketMonitorStreamEventStarted => {
            let info = unsafe { &(*event_info).stream_start_info };
            debug!(
                "Packet monitor stream started with buffer size {} and truncation size {}",
                info.packet_buffer_size_in_bytes, info.truncation_size
            );
        }
        c_api::PacketMonitorStreamEventKind::PacketMonitorStreamEventStopped => {
            let info = unsafe { &(*event_info).stream_stop_info };
            if info.is_fatal_error {
                error!(
                    "Fatal packet monitor stream error with reason: {}",
                    info.reason
                );
            } else {
                debug!("Packet monitor stream stopped with reason: {}", info.reason);
            }
        }
        c_api::PacketMonitorStreamEventKind::PacketMonitorStreamEventFatalError => {
            error!("Fatal packet monitor stream event");
        }
        c_api::PacketMonitorStreamEventKind::PacketMonitorStreamEventProcessInfo => {
            let info = unsafe { &(*event_info).stream_process_info };
            if info.is_warning {
                warn!(
                    "Packet monitor stream warning reason: {}, packet length: {}",
                    info.reason, info.packet_length
                );
            } else {
                error!(
                    "Packet monitor stream error reason: {}, packet length: {}",
                    info.reason, info.packet_length
                );
            }
        }
    }
}

extern "system" fn data_callback(
    context: *mut c_void,
    data: *const c_api::PacketMonitorStreamDataDescriptor,
) {
    let start = std::time::Instant::now();
    let context = unsafe { &mut *(context as UserContext) };

    let descriptor = unsafe { &(*data) };

    let metadata: &c_api::PacketMonitorStreamMetadata = unsafe {
        &*(descriptor.data.add(descriptor.metadata_offset as usize)
            as *const c_api::PacketMonitorStreamMetadata)
    };

    trace!("Packet type: {:?}", metadata.packet_type);

    if descriptor.missed_packet_write_count > 0 {
        warn!(
            "missed writing packets!!: {}",
            descriptor.missed_packet_write_count
        );
    }

    if descriptor.missed_packet_read_count > 0 {
        warn!(
            "missed reading packets!!: {}",
            descriptor.missed_packet_read_count
        );
    }

    let packet_payload =
        unsafe { (descriptor.data as *const u8).add(descriptor.packet_offset as usize) };
    let packet_payload = slice_from_raw_parts(packet_payload, descriptor.packet_length as usize);
    let mut packet_payload_vector = Vec::new();
    packet_payload_vector.extend_from_slice(unsafe { &*packet_payload });

    // trace!("Cloned time: {:?}", start.elapsed().as_nanos());

    let packet = Packet {
        component_id: metadata.component_id,
        payload: match PacketMonitorPacketType::try_from(metadata.packet_type) {
            Ok(packet_type) => match packet_type {
                PacketMonitorPacketType::PktMonPayload_Unknown => {
                    PacketPayload::Unknown(packet_payload_vector)
                }
                PacketMonitorPacketType::PktMonPayload_Ethernet => {
                    PacketPayload::Ethernet(packet_payload_vector)
                }
                PacketMonitorPacketType::PktMonPayload_WiFi => {
                    PacketPayload::WiFi(packet_payload_vector)
                }
                PacketMonitorPacketType::PktMonPayload_IP => {
                    PacketPayload::IP(packet_payload_vector)
                }
                PacketMonitorPacketType::PktMonPayload_HTTP => {
                    PacketPayload::HTTP(packet_payload_vector)
                }
                PacketMonitorPacketType::PktMonPayload_TCP => {
                    PacketPayload::TCP(packet_payload_vector)
                }
                PacketMonitorPacketType::PktMonPayload_UDP => {
                    PacketPayload::UDP(packet_payload_vector)
                }
                PacketMonitorPacketType::PktMonPayload_ARP => {
                    PacketPayload::ARP(packet_payload_vector)
                }
                PacketMonitorPacketType::PktMonPayload_ICMP => {
                    PacketPayload::ICMP(packet_payload_vector)
                }
                PacketMonitorPacketType::PktMonPayload_ESP => {
                    PacketPayload::ESP(packet_payload_vector)
                }
                PacketMonitorPacketType::PktMonPayload_AH => {
                    PacketPayload::AH(packet_payload_vector)
                }
                PacketMonitorPacketType::PktMonPayload_L4Payload => {
                    PacketPayload::L4Payload(packet_payload_vector)
                }
            },
            Err(()) => PacketPayload::Unknown(packet_payload_vector),
        },
    };

    // let sender = context.sender.clone();
    // trace!("lock time: {:?}", start.elapsed().as_nanos());
    context.sender.send(packet).unwrap();
    // trace!("send time: {:?}", start.elapsed().as_nanos());

    #[cfg(feature = "tokio")]
    if let Some(ref notify) = context.notify {
        if let Some(notify) = notify.upgrade() {
            notify.notify_one();
        }
    }

    trace!("Total time: {:?}", start.elapsed().as_nanos());
}

impl RealTimeBackend {
    pub fn new() -> std::io::Result<Self> {
        let api = c_api::PacketMonitorApi::new()?;

        #[cfg(feature = "tokio")]
        let notify = Some(std::sync::Arc::new(tokio::sync::Notify::new()));

        let mut handle = c_api::PacketMonitorHandle::default();
        (api.initialize)(
            c_api::PACKETMONITOR_API_VERSION,
            std::ptr::null_mut(),
            &mut handle,
        )
        .ok()?;
        trace!("Initialized handle {:?}", handle);

        let mut session = c_api::PacketMonitorSession::default();
        (api.create_live_session)(handle, w!("PktMon Rust"), &mut session).ok()?;

        trace!("Created session {:?}", session);

        let (sender, receiver) = mpsc::channel();

        let context_box = Box::new(MonitorContext {
            sender,

            #[cfg(feature = "tokio")]
            notify: notify.clone().map(|n| std::sync::Arc::downgrade(&n)),
        });
        let context_ptr = &*context_box as *const _;

        let stream_config = c_api::PacketMonitorRealTimeStreamConfiguration {
            user_context: context_ptr as *mut c_void,
            event_callback: event_callback as *mut _,
            data_callback: data_callback as *mut _,
            buffer_size_multiplier: 10, // Idk if this is a good default
            truncation_size: 9000,      // Max value
        };

        let mut stream = c_api::PacketMonitorRealTimeStream::default();
        (api.create_realtime_stream)(handle, &stream_config, &mut stream).ok()?;

        trace!("Created stream {:?}", stream);

        install_shutdown_hook(move || {
            if let Ok(api) = c_api::PacketMonitorApi::new() {
                (api.close_realtime_stream)(stream);
                (api.close_session_handle)(session);
                (api.uninitialize)(handle);
            }
        });

        (api.attach_output_to_session)(session, stream).ok()?;

        Ok(RealTimeBackend {
            api,

            handle,
            session,
            stream,

            context: context_box,

            receiver,

            #[cfg(feature = "tokio")]
            notify,

            loaded: true,
        })
    }

    /// The data sources pktmon enumerates.
    fn list_data_sources(&self) -> std::io::Result<Vec<crate::DataSource>> {
        // SAFETY: the buffer holds `len` initialised bytes the API filled.
        self.with_data_source_buffer(Vec::new(), |buffer, len| unsafe {
            read_data_sources(buffer, len)
        })
    }

    /// Narrow the session to the data sources with these ids (pktmon's
    /// `--comp`); each spec is passed to the API as it lies in the
    /// enumeration buffer. Answers how many the API accepted.
    fn add_session_data_sources(&self, ids: &[u32]) -> std::io::Result<usize> {
        self.with_data_source_buffer(0, |buffer, len| {
            let mut added = 0;
            // SAFETY: the buffer holds `len` initialised bytes the API filled,
            // and every entry `data_source_entries` answers is a whole spec in it.
            for at in unsafe { data_source_entries(buffer, len) } {
                let id =
                    unsafe { read_at::<u32>(buffer, len, at + std::mem::offset_of!(Spec, id)) };
                let Some(id) = id.filter(|id| ids.contains(id)) else {
                    continue;
                };
                let spec = unsafe { buffer.add(at) }.cast::<Spec>();
                match (self.api.add_single_data_source_to_session)(self.session, spec).ok() {
                    Ok(()) => added += 1,
                    Err(e) => warn!("pktmon refused data source {id}: {e}"),
                }
            }
            added
        })
    }

    /// Runs `read` over the buffer `PacketMonitorEnumDataSources` fills: a
    /// count, an array of pointers and the entries those point at (all
    /// inside the buffer). It is read field by field with bounds checks,
    /// because reading it through a `&PacketMonitorDataSourceList` whose
    /// pointer array is declared one entry long was undefined behaviour,
    /// and release builds crashed inside ntdll at start. `empty` is the
    /// answer when pktmon lists nothing.
    fn with_data_source_buffer<T>(
        &self,
        empty: T,
        read: impl FnOnce(*const u8, usize) -> T,
    ) -> std::io::Result<T> {
        let mut bytes_needed = 0u32;
        (self.api.enum_data_sources)(
            self.handle,
            c_api::PacketMonitorDataSourceKind::PacketMonitorDataSourceKindAll,
            true,
            0,
            &mut bytes_needed,
            std::ptr::null_mut(),
        )
        .ok()?;
        let capacity = bytes_needed as usize;
        if capacity < std::mem::size_of::<PacketMonitorDataSourceList>() {
            return Ok(empty);
        }
        let layout = Layout::from_size_align(
            capacity,
            std::mem::align_of::<PacketMonitorDataSourceList>(),
        )
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        // SAFETY: the layout has a non-zero size.
        let buffer = unsafe { std::alloc::alloc_zeroed(layout) };
        if buffer.is_null() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::OutOfMemory,
                "data source list",
            ));
        }
        let filled = (self.api.enum_data_sources)(
            self.handle,
            c_api::PacketMonitorDataSourceKind::PacketMonitorDataSourceKindAll,
            true,
            bytes_needed,
            &mut bytes_needed,
            buffer.cast(),
        )
        .ok();
        // `buffer` holds `capacity` initialised bytes the API filled.
        let value = filled.map(|()| read(buffer, capacity));
        // SAFETY: allocated above with this layout, and nothing points into it any more.
        unsafe { std::alloc::dealloc(buffer, layout) };
        Ok(value?)
    }
}

impl CaptureBackend for RealTimeBackend {
    fn start(&mut self) -> std::io::Result<()> {
        info!("Setting session active");
        (self.api.set_session_active)(self.session, true).ok()?;

        Ok(())
    }

    fn stop(&mut self) -> std::io::Result<()> {
        info!("Setting session inactive");
        (self.api.set_session_active)(self.session, false).ok()?;

        Ok(())
    }

    fn unload(&mut self) -> std::io::Result<()> {
        if self.loaded {
            debug!("Closing stream");
            (self.api.close_realtime_stream)(self.stream);

            debug!("Closing session");
            (self.api.close_session_handle)(self.session);

            debug!("Uninitializing");
            (self.api.uninitialize)(self.handle);

            self.loaded = false;
        }

        Ok(())
    }

    fn add_filter(&mut self, filter: PktMonFilter) -> std::io::Result<()> {
        info!("Adding filter");
        (self.api.add_capture_constraint)(self.session, &filter.into()).ok()?;

        Ok(())
    }

    fn data_sources(&self) -> std::io::Result<Vec<crate::DataSource>> {
        self.list_data_sources()
    }

    fn add_data_sources(&mut self, ids: &[u32]) -> std::io::Result<usize> {
        self.add_session_data_sources(ids)
    }

    fn next_packet(&self) -> Result<Packet, mpsc::RecvError> {
        debug!("Receiving packet");
        self.receiver.recv()
    }

    fn next_packet_timeout(&self, timeout: Duration) -> Result<Packet, mpsc::RecvTimeoutError> {
        debug!("Receiving packet with timeout");
        self.receiver.recv_timeout(timeout)
    }

    fn try_next_packet(&self) -> Result<Packet, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }

    #[cfg(feature = "tokio")]
    fn notify(&self) -> Option<std::sync::Arc<tokio::sync::Notify>> {
        self.notify.clone()
    }
}

impl Drop for RealTimeBackend {
    fn drop(&mut self) {
        self.unload().unwrap();
    }
}

/// Read `T` at `at` in `buffer` (`len` bytes), unaligned; `None` past its end.
///
/// # Safety
/// `buffer` must point at `len` initialised bytes, and `T` must be valid for
/// any bit pattern (integers and arrays of them).
unsafe fn read_at<T: Copy>(buffer: *const u8, len: usize, at: usize) -> Option<T> {
    let end = at.checked_add(std::mem::size_of::<T>())?;
    (end <= len).then(|| unsafe { buffer.add(at).cast::<T>().read_unaligned() })
}

/// The offset in the buffer `PacketMonitorEnumDataSources` filled of every
/// entry that lies whole inside it (an entry outside is a layout mismatch,
/// and ends the list).
///
/// # Safety
/// `buffer` must point at `len` initialised bytes.
unsafe fn data_source_entries(buffer: *const u8, len: usize) -> Vec<usize> {
    let base = buffer as usize;
    let count: u32 = unsafe {
        read_at(
            buffer,
            len,
            std::mem::offset_of!(PacketMonitorDataSourceList, num_data_sources),
        )
    }
    .unwrap_or(0);
    let pointers = std::mem::offset_of!(PacketMonitorDataSourceList, data_sources);
    let mut entries = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let Some(pointer) = i
            .checked_mul(std::mem::size_of::<usize>())
            .and_then(|offset| unsafe { read_at::<usize>(buffer, len, pointers + offset) })
        else {
            break;
        };
        let Some(at) = pointer.checked_sub(base).filter(|at| {
            at.checked_add(std::mem::size_of::<Spec>())
                .is_some_and(|end| end <= len)
        }) else {
            break;
        };
        entries.push(at);
    }
    entries
}

/// The data sources in the buffer `PacketMonitorEnumDataSources` filled.
///
/// # Safety
/// `buffer` must point at `len` initialised bytes.
unsafe fn read_data_sources(buffer: *const u8, len: usize) -> Vec<crate::DataSource> {
    let text = |chars: &[u16]| {
        String::from_utf16_lossy(
            &chars[..chars.iter().position(|&c| c == 0).unwrap_or(chars.len())],
        )
    };
    // SAFETY: every entry lies whole inside the buffer, so each field read
    // is in bounds; the fields are integers or arrays of them.
    unsafe { data_source_entries(buffer, len) }
        .into_iter()
        .map(|at| {
            let read = |offset: usize| unsafe { read_at::<u32>(buffer, len, at + offset) };
            let kind = read(std::mem::offset_of!(Spec, kind)).unwrap_or(0);
            let id = read(std::mem::offset_of!(Spec, id)).unwrap_or(0);
            let parent_id = read(std::mem::offset_of!(Spec, parent_id)).unwrap_or(0);
            let is_present = read(std::mem::offset_of!(Spec, is_present)).unwrap_or(0);
            let name: [u16; 64] =
                unsafe { read_at(buffer, len, at + std::mem::offset_of!(Spec, name)) }
                    .unwrap_or([0; 64]);
            let description: [u16; 128] =
                unsafe { read_at(buffer, len, at + std::mem::offset_of!(Spec, description)) }
                    .unwrap_or([0; 128]);
            crate::DataSource {
                id,
                parent_id,
                name: text(&name),
                description: text(&description),
                present: is_present != 0,
                network_interface: kind
                    == c_api::PacketMonitorDataSourceKind::PacketMonitorDataSourceKindNetworkInterface
                        as u32,
            }
        })
        .collect()
}
