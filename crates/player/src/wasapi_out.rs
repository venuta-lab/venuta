//! Windows sound devices through WASAPI.
//!
//! Exclusive mode gives the device each song's own format (rate, depth, channels), so the
//! samples reach the DAC exactly; when the device can't take that format, shared mode hands
//! the song to Windows' mixer.
//!
//! Two outputs live here. Engine 1's `WasapiDevice` (an `output::Device`) runs on the player's
//! own thread, waiting and writing; shared mode lets Windows convert the rate. Output v2's
//! `WasapiOutput` (an `output_api::Output`, section 4.8 of the Listen together design) owns its
//! render thread: `WasapiOpener` opens the device on that thread through the `windows` crate
//! directly (every COM object is made, used and released there), and `output::PullOutput`
//! runs it. Shared mode opens at the mix format (decision D2: the renderer converts).

use crate::decode::SourceFormat;
use crate::output::{Device, DeviceFormat, Mode, SampleKind};
use serde::Serialize;
use wasapi::{
    AudioClient, AudioClock, AudioRenderClient, Direction, Handle, SampleType, StreamMode,
    WaveFormat,
};

/// A sound output the player can use.
#[derive(Debug, Clone, Serialize)]
pub struct OutputDevice {
    pub id: String,
    pub name: String,
    pub default: bool,
}

/// Sound outputs that are plugged in and on.
pub fn list_devices() -> Result<Vec<OutputDevice>, String> {
    let _ = wasapi::initialize_mta();
    let default_id = wasapi::get_default_device(&Direction::Render)
        .and_then(|d| d.get_id())
        .unwrap_or_default();
    let collection =
        wasapi::DeviceCollection::new(&Direction::Render).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for d in &collection {
        let Ok(d) = d else { continue };
        let (Ok(id), Ok(name)) = (d.get_id(), d.get_friendlyname()) else {
            continue;
        };
        out.push(OutputDevice {
            default: id == default_id,
            id,
            name,
        });
    }
    Ok(out)
}

fn find_device(id: Option<&str>) -> Result<wasapi::Device, String> {
    if let Some(id) = id {
        let collection =
            wasapi::DeviceCollection::new(&Direction::Render).map_err(|e| e.to_string())?;
        for d in &collection {
            if let Ok(d) = d
                && d.get_id().ok().as_deref() == Some(id)
            {
                return Ok(d);
            }
        }
    }
    wasapi::get_default_device(&Direction::Render)
        .map_err(|e| format!("No sound output found ({e})."))
}

pub struct WasapiDevice {
    device: wasapi::Device,
    name: String,
    /// Asked for exclusive mode (may still fall back to shared for a format it can't take).
    want_exclusive: bool,
    mode: Mode,
    client: Option<AudioClient>,
    render: Option<AudioRenderClient>,
    event: Option<Handle>,
    buffer_frames: usize,
    /// The device's clock: which frame is reaching the speakers now (for playing in step).
    clock: Option<(AudioClock, u64)>,
    rate: u32,
    /// Frames written since the stream opened.
    written: u64,
}

impl WasapiDevice {
    pub fn new(id: Option<&str>, exclusive: bool) -> Result<Self, String> {
        let _ = wasapi::initialize_mta();
        let device = find_device(id)?;
        let name = device
            .get_friendlyname()
            .unwrap_or_else(|_| "Speakers".into());
        Ok(Self {
            device,
            name,
            want_exclusive: exclusive,
            mode: if exclusive {
                Mode::Exclusive
            } else {
                Mode::Shared
            },
            client: None,
            render: None,
            event: None,
            buffer_frames: 0,
            clock: None,
            rate: 0,
            written: 0,
        })
    }

    /// Formats to offer for exclusive mode, best first: the song's own depth, then deeper
    /// containers that still carry it exactly.
    fn exclusive_candidates(src: &SourceFormat) -> Vec<(usize, usize, SampleType)> {
        match (src.lossless, src.bits) {
            (true, 16) => vec![
                (16, 16, SampleType::Int),
                (32, 32, SampleType::Int),
                (24, 24, SampleType::Int),
            ],
            (true, 24) => vec![
                (32, 24, SampleType::Int),
                (24, 24, SampleType::Int),
                (32, 32, SampleType::Int),
            ],
            (true, _) => vec![(32, 32, SampleType::Int)],
            // Decoded lossy audio: as deep as the device goes.
            (false, _) => vec![
                (32, 32, SampleType::Int),
                (24, 24, SampleType::Int),
                (32, 32, SampleType::Float),
                (16, 16, SampleType::Int),
            ],
        }
    }

    fn try_exclusive(&mut self, src: &SourceFormat) -> Result<DeviceFormat, String> {
        let mut last = String::from("The device takes no format close to this song's.");
        for channels in [src.channels, 2] {
            for (store, valid, kind) in Self::exclusive_candidates(src) {
                let want = WaveFormat::new(
                    store,
                    valid,
                    &kind,
                    src.rate as usize,
                    usize::from(channels),
                    None,
                );
                let mut client = self.device.get_iaudioclient().map_err(|e| e.to_string())?;
                let Ok(fmt) = client.is_supported_exclusive_with_quirks(&want) else {
                    continue;
                };
                let (_, min_period) = client.get_device_period().map_err(|e| e.to_string())?;
                // About 10 ms: short enough to answer quickly, long enough to be steady.
                let period = client
                    .calculate_aligned_period_near(min_period.max(100_000), Some(128), &fmt)
                    .unwrap_or(min_period.max(100_000));
                let mode = StreamMode::EventsExclusive { period_hns: period };
                let init = client.initialize_client(&fmt, &Direction::Render, &mode);
                if let Err(e) = init {
                    // Some drivers want the buffer aligned their way: ask, then try once more.
                    let retry = client.get_buffer_size().ok().map(|frames| {
                        wasapi::calculate_period_100ns(
                            i64::from(frames),
                            fmt.get_samplespersec() as i64,
                        )
                    });
                    match retry {
                        Some(p) => {
                            client = self.device.get_iaudioclient().map_err(|e| e.to_string())?;
                            if let Err(e2) = client.initialize_client(
                                &fmt,
                                &Direction::Render,
                                &StreamMode::EventsExclusive { period_hns: p },
                            ) {
                                last = format!("{e2}");
                                continue;
                            }
                        }
                        None => {
                            last = format!("{e}");
                            continue;
                        }
                    }
                }
                return self.finish_open(client, &fmt, Mode::Exclusive);
            }
        }
        Err(last)
    }

    fn open_shared(&mut self, src: &SourceFormat) -> Result<DeviceFormat, String> {
        let mut client = self.device.get_iaudioclient().map_err(|e| e.to_string())?;
        // Float at the song's own rate; Windows converts to its mix format.
        let fmt = WaveFormat::new(
            32,
            32,
            &SampleType::Float,
            src.rate as usize,
            usize::from(src.channels.clamp(1, 2)),
            None,
        );
        let (def, _) = client.get_device_period().map_err(|e| e.to_string())?;
        client
            .initialize_client(
                &fmt,
                &Direction::Render,
                &StreamMode::EventsShared {
                    autoconvert: true,
                    // 50 ms: a music player needs no low latency, and a bigger buffer rides
                    // out a late period (the latency is measured, not guessed, for a room).
                    buffer_duration_hns: def.max(500_000),
                },
            )
            .map_err(|e| format!("The sound output refused to open ({e})."))?;
        self.finish_open(client, &fmt, Mode::Shared)
    }

    fn finish_open(
        &mut self,
        client: AudioClient,
        fmt: &WaveFormat,
        mode: Mode,
    ) -> Result<DeviceFormat, String> {
        let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
        let render = client.get_audiorenderclient().map_err(|e| e.to_string())?;
        self.buffer_frames = client.get_buffer_size().map_err(|e| e.to_string())? as usize;
        if std::env::var_os("VENUTA_DEBUG_WASAPI").is_some() {
            eprintln!(
                "wasapi {mode:?}: {} frames, blockalign {}, {} bits in {}",
                self.buffer_frames,
                fmt.get_blockalign(),
                fmt.get_validbitspersample(),
                fmt.get_bitspersample()
            );
        }
        self.clock = client
            .get_audioclock()
            .ok()
            .and_then(|c| c.get_frequency().ok().filter(|f| *f > 0).map(|f| (c, f)));
        self.rate = fmt.get_samplespersec();
        self.written = 0;
        self.client = Some(client);
        self.render = Some(render);
        self.event = Some(event);
        self.mode = mode;
        Ok(DeviceFormat {
            rate: fmt.get_samplespersec(),
            channels: fmt.get_nchannels(),
            container_bits: fmt.get_bitspersample(),
            valid_bits: fmt.get_validbitspersample(),
            kind: match fmt.get_subformat() {
                Ok(SampleType::Float) => SampleKind::Float,
                _ => SampleKind::Int,
            },
        })
    }
}

impl Device for WasapiDevice {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn mode(&self) -> Mode {
        self.mode
    }
    fn open(&mut self, src: &SourceFormat) -> Result<DeviceFormat, String> {
        self.close();
        if self.want_exclusive {
            match self.try_exclusive(src) {
                Ok(f) => return Ok(f),
                // In use by another app, or no matching format: share instead of failing.
                Err(e) => {
                    if std::env::var_os("VENUTA_DEBUG_WASAPI").is_some() {
                        eprintln!("wasapi: exclusive refused ({e}); sharing instead");
                    }
                    return self.open_shared(src);
                }
            }
        }
        self.open_shared(src)
    }
    fn available(&mut self) -> Result<usize, String> {
        let c = self.client.as_ref().ok_or("The output isn't open.")?;
        if self.mode == Mode::Exclusive {
            // Exclusive, event-driven: the whole buffer each time.
            return Ok(self.buffer_frames);
        }
        c.get_available_space_in_frames()
            .map(|n| n as usize)
            .map_err(|e| e.to_string())
    }
    fn wait(&mut self) -> Result<usize, String> {
        let ev = self.event.as_ref().ok_or("The output isn't open.")?;
        ev.wait_for_event(2000)
            .map_err(|_| "The sound output stopped responding.".to_string())?;
        self.available()
    }
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<(), String> {
        if frames == 0 {
            return Ok(());
        }
        let r = self.render.as_ref().ok_or("The output isn't open.")?;
        r.write_to_device(frames, bytes, None)
            .map_err(|e| format!("The sound output stopped ({e})."))?;
        self.written += frames as u64;
        Ok(())
    }
    fn latency_frames(&self) -> usize {
        self.buffer_frames
    }
    fn flush(&mut self) {
        if let Some(c) = self.client.as_ref()
            && c.reset_stream().is_ok()
        {
            // The clock starts again from nothing, with what's written.
            self.written = 0;
        }
    }
    fn queued_frames(&self) -> Option<usize> {
        let (clock, freq) = self.clock.as_ref()?;
        // The position comes with the moment it was read at (the performance counter, in
        // 100 ns units): what has played since then is counted on, so a driver that reports
        // its position only every millisecond or ten is no noise in the measure.
        let (pos, at) = clock.get_position().ok()?;
        let mut heard = u128::from(pos) * u128::from(self.rate) / u128::from(*freq);
        if at > 0
            && let Some(now) = qpc_100ns()
            && now > at
        {
            heard += u128::from(now - at) * u128::from(self.rate) / 10_000_000;
        }
        Some(self.written.saturating_sub(heard as u64) as usize)
    }
    fn start(&mut self) -> Result<(), String> {
        self.client
            .as_ref()
            .ok_or("The output isn't open.")?
            .start_stream()
            .map_err(|e| e.to_string())
    }
    fn stop(&mut self) -> Result<(), String> {
        if let Some(c) = self.client.as_ref() {
            c.stop_stream().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn close(&mut self) {
        if let Some(c) = self.client.as_ref() {
            let _ = c.stop_stream();
        }
        self.render = None;
        self.event = None;
        self.clock = None;
        self.client = None;
    }
}

/// The performance counter now, in 100 ns units (what `IAudioClock::GetPosition` stamps its
/// position with).
fn qpc_100ns() -> Option<u64> {
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let (mut count, mut freq) = (0i64, 0i64);
    // SAFETY: plain calls with valid out pointers.
    unsafe {
        QueryPerformanceCounter(&mut count).ok()?;
        QueryPerformanceFrequency(&mut freq).ok()?;
    }
    (freq > 0).then(|| (u128::from(count as u64) * 10_000_000 / u128::from(freq as u64)) as u64)
}

/// This thread, scheduled by Windows as pro audio (the Multimedia Class Scheduler), while it
/// lasts: ahead of ordinary work, so a busy computer doesn't make the sound late.
pub struct ProAudio(Option<windows::Win32::Foundation::HANDLE>);

impl ProAudio {
    pub fn enter() -> Self {
        let mut task = 0u32;
        // SAFETY: a plain call with a valid task name and out pointer; the handle it returns is
        // given back on drop, on this same thread.
        let h = unsafe {
            windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW(
                windows::core::w!("Pro Audio"),
                &mut task,
            )
        };
        ProAudio(h.ok())
    }

    /// Whether Windows took it (the scheduler service can be off).
    pub fn active(&self) -> bool {
        self.0.is_some()
    }
}

impl Drop for ProAudio {
    fn drop(&mut self) {
        if let Some(h) = self.0.take() {
            // SAFETY: the handle came from AvSetMmThreadCharacteristicsW on this thread.
            let _ =
                unsafe { windows::Win32::System::Threading::AvRevertMmThreadCharacteristics(h) };
        }
    }
}

// --- Output v2 ------------------------------------------------------------------------------

use crate::output::{
    DeviceNotes, EventSink, Fault, Kick, Opened, PositionMap, PullDevice, PullOpener, PullOutput,
    Space, Wake,
};
use crate::output_api::{OpenRequest, OutputError, OutputEvent, OutputInfo};
use std::sync::Arc;
use together::time::{RealClock, Sleeper};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Media::Audio::{
    AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_DEVICE_INVALIDATED,
    AUDCLNT_E_SERVICE_NOT_RUNNING, AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, DEVICE_STATE, DEVICE_STATE_ACTIVE, EDataFlow, ERole,
    IAudioClient, IAudioClock, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::Media::KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE};
use windows::Win32::Media::Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};
use windows::core::{HRESULT, HSTRING, Interface, PCWSTR, implement};

/// The output v2 on WASAPI.
pub type WasapiOutput = PullOutput<WasapiOpener>;

/// An output on the endpoint `id` (`None`: the system's default output, followed: a change of
/// default is told as `DefaultChanged`), exclusive mode if asked and the device takes it.
pub fn wasapi_output(id: Option<&str>, exclusive: bool) -> WasapiOutput {
    PullOutput::new(
        WasapiOpener {
            id: id.map(str::to_owned),
            exclusive,
        },
        Arc::new(RealClock),
    )
}

/// Opens a WASAPI endpoint for `PullOutput`, on its render thread.
#[derive(Debug, Clone)]
pub struct WasapiOpener {
    id: Option<String>,
    exclusive: bool,
}

/// The shared-mode buffer asked for: 50 ms (section 4.8).
const SHARED_BUFFER_HNS: i64 = 500_000;
/// The exclusive-mode period aimed at: 10 ms.
const EXCLUSIVE_PERIOD_HNS: i64 = 100_000;
/// Intel HDA (and so most devices) wants buffers in multiples of 128 bytes.
const ALIGN_BYTES: u32 = 128;

impl PullOpener for WasapiOpener {
    type Device = WasapiPull;

    fn name(&self) -> String {
        self.id.clone().unwrap_or_else(|| "Default output".into())
    }

    fn mode(&self) -> Mode {
        if self.exclusive {
            Mode::Exclusive
        } else {
            Mode::Shared
        }
    }

    fn open(
        &mut self,
        want: &OpenRequest,
        sink: &Arc<EventSink>,
    ) -> Result<Opened<WasapiPull>, OutputError> {
        open_pull(self, want, sink)
    }
}

/// COM on this thread (multithreaded), while it lasts.
struct Com(bool);

impl Com {
    fn init() -> Com {
        // SAFETY: a plain call; S_OK and S_FALSE (already initialised) are each matched by one
        // CoUninitialize in drop, on this same thread.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Com(hr.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: matches the CoInitializeEx that succeeded on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// An auto-reset Windows event.
struct Event(HANDLE);

// SAFETY: an event is a kernel handle; any thread may signal it or wait on it, and it is
// closed once, when the last owner drops it.
unsafe impl Send for Event {}
// SAFETY: as above; signalling and waiting need no exclusive access.
unsafe impl Sync for Event {}

impl Event {
    fn new() -> Result<Event, OutputError> {
        // SAFETY: a plain call: no attributes, auto-reset, not signalled, unnamed.
        unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
            .map(Event)
            .map_err(|e| OutputError::Other(e.to_string()))
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateEventW and is closed only here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Wakes the render thread: its kick event.
struct KickEvent(Arc<Event>);

impl Kick for KickEvent {
    fn kick(&self) {
        // SAFETY: a valid event handle, alive while this Arc is.
        let _ = unsafe { SetEvent(self.0.0) };
    }
}

/// The notification client: our endpoint gone or no longer active (`Lost`), or a new default
/// output while we follow the default (`DefaultChanged`). Windows calls it on its own threads.
#[implement(IMMNotificationClient)]
struct Notifier {
    id: Option<String>,
    follow_default: bool,
    sink: Arc<EventSink>,
}

impl Notifier {
    fn ours(&self, id: &PCWSTR) -> bool {
        if id.is_null() {
            return false;
        }
        // SAFETY: Windows passes a valid, terminated string for the call's length.
        let id = unsafe { id.to_string() }.ok();
        self.id.is_some() && id == self.id
    }
}

impl IMMNotificationClient_Impl for Notifier_Impl {
    fn OnDeviceStateChanged(&self, id: &PCWSTR, state: DEVICE_STATE) -> windows::core::Result<()> {
        if state != DEVICE_STATE_ACTIVE && self.ours(id) {
            self.sink.send(OutputEvent::Lost);
        }
        Ok(())
    }

    fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, id: &PCWSTR) -> windows::core::Result<()> {
        if self.ours(id) {
            self.sink.send(OutputEvent::Lost);
        }
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        id: &PCWSTR,
    ) -> windows::core::Result<()> {
        // The console role is the one we open; a default set to the endpoint we already have
        // (or none left: then `Lost` follows) is no change for us.
        if self.follow_default && flow == eRender && role == eConsole && !self.ours(id) {
            self.sink.send(OutputEvent::DefaultChanged);
        }
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _id: &PCWSTR,
        _key: &windows::Win32::Foundation::PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

/// The notification client's registration, undone on drop (on the render thread).
struct Registration {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
}

impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: the client registered with this enumerator in `open_pull`.
        let _ = unsafe {
            self.enumerator
                .UnregisterEndpointNotificationCallback(&self.client)
        };
    }
}

/// An open WASAPI stream, on its render thread. Fields drop in order: the notification
/// registration and the COM objects first, then the events, MMCSS and COM itself.
pub struct WasapiPull {
    _notify: Option<Registration>,
    clock: IAudioClock,
    render: IAudioRenderClient,
    client: IAudioClient,
    audio: Event,
    kick: Arc<Event>,
    bpf: usize,
    rate: u32,
    freq: u64,
    /// The last position, in the clock'"'"'s units (for traces).
    last_units: u64,
    buffer: u32,
    exclusive: bool,
    _pro: ProAudio,
    _com: Com,
}

/// A device call's error: a lost device, or anything else.
fn fault(e: &windows::core::Error) -> Fault {
    fault_of(e.code(), &e.message())
}

fn fault_of(hr: HRESULT, what: &str) -> Fault {
    if hr == AUDCLNT_E_DEVICE_INVALIDATED || hr == AUDCLNT_E_SERVICE_NOT_RUNNING {
        Fault::Lost
    } else {
        Fault::Other(format!("{what} ({:#010x})", hr.0 as u32))
    }
}

/// An open's error.
fn open_error(e: &windows::core::Error) -> OutputError {
    match fault(e) {
        Fault::Lost => OutputError::Lost,
        Fault::Other(_) if e.code() == AUDCLNT_E_DEVICE_IN_USE => OutputError::Busy,
        Fault::Other(w) => OutputError::Other(format!("The sound output refused to open: {w}.")),
    }
}

impl PullDevice for WasapiPull {
    fn wait(&mut self, _sleeper: &Sleeper, running: bool, timeout_ns: u64) -> Result<Wake, Fault> {
        let ms = u32::try_from(timeout_ns / 1_000_000).unwrap_or(u32::MAX - 1);
        let both = [self.audio.0, self.kick.0];
        let handles = if running { &both[..] } else { &both[1..] };
        // SAFETY: valid event handles, alive for the call.
        let r = unsafe { WaitForMultipleObjects(handles, false, ms) };
        if r == WAIT_TIMEOUT {
            return Ok(Wake::Timeout);
        }
        let k = r.0.wrapping_sub(WAIT_OBJECT_0.0);
        match (running, k) {
            (true, 0) => Ok(Wake::Device),
            (true, 1) | (false, 0) => Ok(Wake::Kick),
            _ => Err(Fault::Other(format!(
                "waiting for the device failed ({:#x})",
                r.0
            ))),
        }
    }

    fn space(&mut self) -> Result<Space, Fault> {
        if self.exclusive {
            // Exclusive, event driven: the whole buffer at each event.
            return Ok(Space {
                free: self.buffer,
                held: None,
            });
        }
        // SAFETY: a plain call on an initialised client.
        let held = unsafe { self.client.GetCurrentPadding() }.map_err(|e| fault(&e))?;
        Ok(Space {
            free: self.buffer.saturating_sub(held),
            held: Some(held),
        })
    }

    fn write(&mut self, frames: u32, fill: &mut dyn FnMut(&mut [u8])) -> Result<(), Fault> {
        // SAFETY: GetBuffer gives `frames` frames of the stream's format, ours until
        // ReleaseBuffer; the slice covers exactly them and is gone before the release.
        unsafe {
            let p = self.render.GetBuffer(frames).map_err(|e| fault(&e))?;
            fill(std::slice::from_raw_parts_mut(
                p,
                frames as usize * self.bpf,
            ));
            self.render.ReleaseBuffer(frames, 0).map_err(|e| fault(&e))
        }
    }

    fn position(&mut self) -> Result<Option<(u64, u64)>, Fault> {
        let (mut pos, mut qpc) = (0u64, 0u64);
        // Through the vtable, to see the HRESULT itself: only S_OK is a reading (S_FALSE and any
        // other success code are not).
        // SAFETY: a valid IAudioClock and two valid out pointers.
        let hr = unsafe {
            (Interface::vtable(&self.clock).GetPosition)(
                Interface::as_raw(&self.clock),
                &mut pos,
                &mut qpc,
            )
        };
        if hr.is_err() {
            return Err(fault_of(hr, "reading the device's position failed"));
        }
        if hr != windows::Win32::Foundation::S_OK {
            return Ok(None);
        }
        self.last_units = pos;
        // The stamp is the performance counter in 100 ns units: the same counter as RealClock.
        Ok(Some((
            frames_of(pos, self.freq, self.rate),
            qpc.saturating_mul(100),
        )))
    }

    fn start(&mut self) -> Result<(), Fault> {
        // SAFETY: a plain call on an initialised client.
        unsafe { self.client.Start() }.map_err(|e| fault(&e))
    }

    fn halt(&mut self) -> Result<(), Fault> {
        // SAFETY: plain calls on an initialised client (Stop on a stopped stream is S_FALSE).
        unsafe {
            self.client.Stop().map_err(|e| fault(&e))?;
            self.client.Reset().map_err(|e| fault(&e))
        }
    }

    fn raw_clock(&self) -> (u64, u64) {
        (self.last_units, self.freq)
    }

    fn probe_padding(&mut self) -> Option<u32> {
        // SAFETY: a plain call on an initialised client.
        unsafe { self.client.GetCurrentPadding() }.ok()
    }
}

/// An initialised client and what it runs at.
struct Init {
    client: IAudioClient,
    format: DeviceFormat,
    exclusive: bool,
}

fn open_pull(
    opener: &WasapiOpener,
    want: &OpenRequest,
    sink: &Arc<EventSink>,
) -> Result<Opened<WasapiPull>, OutputError> {
    let com = Com::init();
    let pro = ProAudio::enter();
    // SAFETY: COM is initialised on this thread; plain calls with valid arguments.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| open_error(&e))?;
        let device: IMMDevice = match &opener.id {
            Some(id) => enumerator.GetDevice(&HSTRING::from(id.as_str())),
            None => enumerator.GetDefaultAudioEndpoint(eRender, eConsole),
        }
        .map_err(|_| OutputError::Lost)?;
        let endpoint_id = device.GetId().ok().and_then(|p| {
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        });
        let name = wasapi::Device::from_immdevice(device.clone())
            .and_then(|d| d.get_friendlyname())
            .unwrap_or_else(|_| "Speakers".into());
        let init = if opener.exclusive {
            match open_exclusive(&device, want) {
                Ok(i) => i,
                // Another app has it, or no format fits: share instead (H5).
                Err(_) => open_shared(&device)?,
            }
        } else {
            open_shared(&device)?
        };
        let client = init.client;
        let rate = init.format.rate;
        let audio = Event::new()?;
        client.SetEventHandle(audio.0).map_err(|e| open_error(&e))?;
        let render: IAudioRenderClient = client.GetService().map_err(|e| open_error(&e))?;
        let clock: IAudioClock = client.GetService().map_err(|e| open_error(&e))?;
        let freq = clock.GetFrequency().map_err(|e| open_error(&e))?.max(1);
        let buffer = client.GetBufferSize().map_err(|e| open_error(&e))?;
        let latency = client
            .GetStreamLatency()
            .map(|hns| frames_of_hns(hns, rate))
            .unwrap_or(0);
        let (mut default_hns, mut min_hns) = (0i64, 0i64);
        client
            .GetDevicePeriod(Some(&mut default_hns), Some(&mut min_hns))
            .map_err(|e| open_error(&e))?;
        let (info, map, mode) = if init.exclusive {
            (
                OutputInfo {
                    format: init.format.clone(),
                    period_frames: buffer,
                    buffer_frames: 2 * buffer + latency,
                },
                PositionMap::exclusive(rate, buffer),
                Mode::Exclusive,
            )
        } else {
            let period = frames_of_hns(default_hns, rate).clamp(1, buffer.max(1));
            (
                OutputInfo {
                    format: init.format.clone(),
                    period_frames: period,
                    buffer_frames: buffer + latency,
                },
                PositionMap::shared(rate, period),
                Mode::Shared,
            )
        };
        let notifier: IMMNotificationClient = Notifier {
            id: endpoint_id.clone(),
            follow_default: opener.id.is_none(),
            sink: sink.clone(),
        }
        .into();
        // Without notifications the output still works (a lost device is seen in its calls).
        let notify = enumerator
            .RegisterEndpointNotificationCallback(&notifier)
            .ok()
            .map(|()| Registration {
                enumerator,
                client: notifier,
            });
        let kick = Arc::new(Event::new()?);
        if std::env::var_os("VENUTA_DEBUG_WASAPI").is_some() {
            eprintln!(
                "wasapi output {mode:?}: {:?}, buffer {buffer}, period {}, latency {latency}, pro audio {}",
                init.format,
                info.period_frames,
                pro.active()
            );
        }
        let notes = DeviceNotes {
            endpoint_id,
            device_buffer_frames: buffer,
            stream_latency_frames: latency,
            pro_audio: pro.active(),
        };
        Ok(Opened {
            device: WasapiPull {
                _notify: notify,
                clock,
                render,
                client,
                audio,
                kick: kick.clone(),
                bpf: init.format.bytes_per_frame(),
                rate,
                freq,
                last_units: 0,
                buffer,
                exclusive: init.exclusive,
                _pro: pro,
                _com: com,
            },
            info,
            mode,
            name,
            kick: Arc::new(KickEvent(kick)),
            map,
            notes,
        })
    }
}

/// Shared mode at the mix format (D2), event driven, 50 ms of buffer.
///
/// # Safety
/// COM must be initialised on this thread.
unsafe fn open_shared(device: &IMMDevice) -> Result<Init, OutputError> {
    // SAFETY: as the caller promised; the mix format is read while it lives and freed once.
    unsafe {
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| open_error(&e))?;
        let mix = client.GetMixFormat().map_err(|e| open_error(&e))?;
        // A format the output can write is opened as it is; either way the mix format is freed.
        let opened = format_of_raw(mix).and_then(|format| {
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    SHARED_BUFFER_HNS,
                    0,
                    mix,
                    None,
                )
                .map_err(|e| open_error(&e))
                .map(|()| format)
        });
        CoTaskMemFree(Some(mix as *const _));
        opened.map(|format| Init {
            client,
            format,
            exclusive: false,
        })
    }
}

/// Exclusive mode, the formats best first, each with the drivers' quirks, a period near 10 ms
/// aligned to 128 bytes, and the aligned-period retry.
///
/// # Safety
/// COM must be initialised on this thread.
unsafe fn open_exclusive(device: &IMMDevice, want: &OpenRequest) -> Result<Init, String> {
    let mut last = String::from("The device takes no format close to this song's.");
    let mut channels = vec![want.channels.max(1)];
    if want.channels != 2 {
        channels.push(2);
    }
    for ch in channels {
        for (store, valid, kind) in exclusive_candidates(want.bits) {
            // SAFETY: as the caller promised.
            match unsafe { try_exclusive(device, want.rate, ch, store, valid, kind) } {
                Ok(Some(i)) => return Ok(i),
                Ok(None) => {}
                Err(e) => last = e,
            }
        }
    }
    Err(last)
}

/// One exclusive format: `None` when the device doesn't take it.
///
/// # Safety
/// COM must be initialised on this thread.
unsafe fn try_exclusive(
    device: &IMMDevice,
    rate: u32,
    ch: u16,
    store: u16,
    valid: u16,
    kind: SampleKind,
) -> Result<Option<Init>, String> {
    let st = match kind {
        SampleKind::Int => SampleType::Int,
        SampleKind::Float => SampleType::Float,
    };
    let asked = WaveFormat::new(
        usize::from(store),
        usize::from(valid),
        &st,
        rate as usize,
        usize::from(ch),
        None,
    );
    // SAFETY: as the caller promised; every format pointer is to a live WaveFormat.
    unsafe {
        let activate = || -> Result<IAudioClient, String> {
            device.Activate(CLSCTX_ALL, None).map_err(|e| e.to_string())
        };
        let client = activate()?;
        // The quirks: as given; as a plain WAVEFORMATEX for 1 or 2 channels; with each of the
        // standard channel masks.
        let mut tries = vec![asked.clone()];
        if ch <= 2
            && let Ok(plain) = asked.to_waveformatex()
        {
            tries.push(plain);
        }
        for mask in wasapi::make_channelmasks(usize::from(ch)) {
            tries.push(WaveFormat::new(
                usize::from(store),
                usize::from(valid),
                &st,
                rate as usize,
                usize::from(ch),
                Some(mask),
            ));
        }
        let Some(fmt) = tries.into_iter().find(|f| {
            client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, f.as_waveformatex_ref(), None)
                == windows::Win32::Foundation::S_OK
        }) else {
            return Ok(None);
        };
        let (mut _default, mut min) = (0i64, 0i64);
        client
            .GetDevicePeriod(Some(&mut _default), Some(&mut min))
            .map_err(|e| e.to_string())?;
        let period = aligned_period(
            EXCLUSIVE_PERIOD_HNS,
            min,
            fmt.get_blockalign(),
            rate,
            ALIGN_BYTES,
        );
        let init = |c: &IAudioClient, p: i64| {
            c.Initialize(
                AUDCLNT_SHAREMODE_EXCLUSIVE,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                p,
                p,
                fmt.as_waveformatex_ref(),
                None,
            )
        };
        let format = DeviceFormat {
            rate,
            channels: ch,
            container_bits: store,
            valid_bits: valid,
            kind,
        };
        match init(&client, period) {
            Ok(()) => Ok(Some(Init {
                client,
                format,
                exclusive: true,
            })),
            Err(e) if e.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED => {
                // The driver wants its own alignment: ask it, and try once more on a new client.
                let frames = client.GetBufferSize().map_err(|e| e.to_string())?;
                drop(client);
                let client = activate()?;
                let p = hns_of_frames(frames, rate);
                init(&client, p).map_err(|e| e.to_string())?;
                Ok(Some(Init {
                    client,
                    format,
                    exclusive: true,
                }))
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Formats to offer for exclusive mode, best first, by the bits that carry the signal:
/// (container bits, valid bits, kind).
pub fn exclusive_candidates(bits: u16) -> Vec<(u16, u16, SampleKind)> {
    use SampleKind::{Float, Int};
    match bits {
        16 => vec![(16, 16, Int), (32, 32, Int), (24, 24, Int)],
        24 => vec![(32, 24, Int), (24, 24, Int), (32, 32, Int)],
        // 32 bits (and decoded lossy audio): as deep as the device goes.
        _ => vec![(32, 32, Int), (24, 24, Int), (32, 32, Float), (16, 16, Int)],
    }
}

/// A period near `desired` (100 ns units), at least the device's `min`, whose buffer is a
/// whole number of frames and of `align_bytes` (the same arithmetic as the `wasapi` crate's
/// `calculate_aligned_period_near`).
pub fn aligned_period(
    desired: i64,
    min: i64,
    block_align: u32,
    rate: u32,
    align_bytes: u32,
) -> i64 {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 { a } else { gcd(b, a % b) }
    }
    let desired = desired.max(min);
    let block = block_align.max(1);
    let align = if align_bytes <= 1 {
        block
    } else {
        block / gcd(block, align_bytes) * align_bytes
    };
    let align_frames = i64::from(align / block);
    let desired_frames = (desired as f64 * f64::from(rate) / 1e7).round() as i64;
    let min_frames = (min as f64 * f64::from(rate) / 1e7).ceil() as i64;
    let mut n = desired_frames / align_frames;
    if n * align_frames < min_frames {
        n += 1;
    }
    hns_of_frames((align_frames * n) as u32, rate)
}

/// `frames` at `rate`, in 100 ns units, rounded.
pub fn hns_of_frames(frames: u32, rate: u32) -> i64 {
    (1e7 / f64::from(rate.max(1)) * f64::from(frames) + 0.5) as i64
}

/// `hns` (100 ns units) at `rate`, in frames, rounded.
pub fn frames_of_hns(hns: i64, rate: u32) -> u32 {
    (hns.max(0) as f64 * f64::from(rate) / 1e7).round() as u32
}

/// A device position in the clock's own units (`freq` per second) in frames at `rate`.
pub fn frames_of(pos: u64, freq: u64, rate: u32) -> u64 {
    (u128::from(pos) * u128::from(rate) / u128::from(freq.max(1))) as u64
}

/// The device format of a mix format: 16, 24 (packed, or in 32) and 32-bit integers, 32-bit
/// float, 1 to 8 channels; anything else the output can't write.
pub fn device_format(
    rate: u32,
    channels: u16,
    container_bits: u16,
    valid_bits: u16,
    float: bool,
) -> Result<DeviceFormat, OutputError> {
    let unsupported = || {
        OutputError::Unsupported(format!(
            "The sound output's own format ({container_bits}-bit{}, {channels} channels) isn't one Venuta can write.",
            if float { " float" } else { "" }
        ))
    };
    if !(1..=crate::voice_api::MAX_CH as u16).contains(&channels) || rate == 0 {
        return Err(unsupported());
    }
    let valid = if valid_bits == 0 {
        container_bits
    } else {
        valid_bits
    };
    let kind = match (float, container_bits) {
        (true, 32) if valid == 32 => SampleKind::Float,
        (false, 16) if valid == 16 => SampleKind::Int,
        (false, 24 | 32) if (16..=container_bits).contains(&valid) => SampleKind::Int,
        _ => return Err(unsupported()),
    };
    Ok(DeviceFormat {
        rate,
        channels,
        container_bits,
        valid_bits: valid,
        kind,
    })
}

/// The device format of a `WAVEFORMATEX` (or the `WAVEFORMATEXTENSIBLE` it starts).
///
/// # Safety
/// `f` points to a valid format of the size its own `cbSize` says.
unsafe fn format_of_raw(f: *const WAVEFORMATEX) -> Result<DeviceFormat, OutputError> {
    // SAFETY: as the caller promised; the structs are packed, so fields are copied out.
    let wf = unsafe { f.read_unaligned() };
    let (tag, size) = (u32::from(wf.wFormatTag), wf.cbSize);
    let (float, valid) = if tag == WAVE_FORMAT_EXTENSIBLE && size >= 22 {
        // SAFETY: an extensible format, as its tag and size say.
        let ext = unsafe { (f as *const WAVEFORMATEXTENSIBLE).read_unaligned() };
        let sub = ext.SubFormat;
        let samples = ext.Samples;
        // SAFETY: every member of the union is a u16; this one is the valid bits for PCM.
        let valid = unsafe { samples.wValidBitsPerSample };
        if sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
            (true, valid)
        } else if sub == KSDATAFORMAT_SUBTYPE_PCM {
            (false, valid)
        } else {
            return Err(OutputError::Unsupported(
                "The sound output's own format isn't PCM.".into(),
            ));
        }
    } else {
        (tag == WAVE_FORMAT_IEEE_FLOAT, wf.wBitsPerSample)
    };
    device_format(
        wf.nSamplesPerSec,
        wf.nChannels,
        wf.wBitsPerSample,
        valid,
        float,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_formats_come_best_first() {
        use SampleKind::{Float, Int};
        assert_eq!(
            exclusive_candidates(16),
            [(16, 16, Int), (32, 32, Int), (24, 24, Int)]
        );
        assert_eq!(
            exclusive_candidates(24),
            [(32, 24, Int), (24, 24, Int), (32, 32, Int)]
        );
        assert_eq!(
            exclusive_candidates(32),
            [(32, 32, Int), (24, 24, Int), (32, 32, Float), (16, 16, Int)]
        );
    }

    #[test]
    fn exclusive_periods_are_aligned_as_the_wasapi_crate_aligns_them() {
        // 48 kHz, stereo 32-bit (8 bytes a frame), 128-byte alignment: 16 frames; 480 is 30 of
        // them, so 10 ms as asked.
        assert_eq!(aligned_period(100_000, 30_000, 8, 48_000, 128), 100_000);
        // 44.1 kHz: 441 frames round down to 27 x 16 = 432 frames.
        assert_eq!(
            aligned_period(100_000, 30_000, 8, 44_100, 128),
            hns_of_frames(432, 44_100)
        );
        // 24-bit packed stereo (6 bytes): lcm(6, 128) = 384 bytes, 64 frames; 480 is 7.5 of
        // them: 7 x 64 = 448.
        assert_eq!(
            aligned_period(100_000, 30_000, 6, 48_000, 128),
            hns_of_frames(448, 48_000)
        );
        // Never under the device's minimum: 3 ms minimum, 1 ms asked: one 16-frame step over.
        assert_eq!(
            aligned_period(10_000, 30_000, 8, 48_000, 128),
            hns_of_frames(144, 48_000)
        );
        assert_eq!(hns_of_frames(480, 48_000), 100_000);
        assert_eq!(frames_of_hns(500_000, 48_000), 2_400);
        assert_eq!(frames_of_hns(100_000, 44_100), 441);
    }

    #[test]
    fn positions_in_the_clocks_units_become_frames() {
        // Some clocks count bytes per second, some 10 MHz, some frames.
        assert_eq!(frames_of(384_000, 384_000, 48_000), 48_000);
        assert_eq!(frames_of(10_000_000, 10_000_000, 44_100), 44_100);
        assert_eq!(frames_of(123, 48_000, 48_000), 123);
        assert_eq!(
            frames_of(u64::MAX / 2, 10_000_000, 192_000),
            u64::MAX / 2 / 10_000_000 * 192_000
                + (u64::MAX / 2 % 10_000_000) * 192_000 / 10_000_000
        );
    }

    #[test]
    fn mix_formats_the_output_can_write_and_those_it_cant() {
        let f = device_format(48_000, 2, 32, 32, true).unwrap();
        assert_eq!(
            (f.kind, f.container_bits, f.valid_bits),
            (SampleKind::Float, 32, 32)
        );
        let f = device_format(44_100, 2, 32, 24, false).unwrap();
        assert_eq!(
            (f.kind, f.container_bits, f.valid_bits),
            (SampleKind::Int, 32, 24)
        );
        assert_eq!(device_format(48_000, 6, 24, 24, false).unwrap().channels, 6);
        assert_eq!(
            device_format(48_000, 2, 16, 0, false).unwrap().valid_bits,
            16
        );
        for bad in [
            device_format(48_000, 2, 64, 64, true),
            device_format(48_000, 2, 8, 8, false),
            device_format(48_000, 9, 32, 32, true),
            device_format(48_000, 2, 16, 12, false),
            device_format(0, 2, 32, 32, true),
        ] {
            assert!(matches!(bad, Err(OutputError::Unsupported(_))), "{bad:?}");
        }
    }

    #[test]
    fn a_lost_device_is_told_apart_from_other_errors() {
        assert_eq!(fault_of(AUDCLNT_E_DEVICE_INVALIDATED, "x"), Fault::Lost);
        assert_eq!(fault_of(AUDCLNT_E_SERVICE_NOT_RUNNING, "x"), Fault::Lost);
        assert!(matches!(
            fault_of(AUDCLNT_E_DEVICE_IN_USE, "x"),
            Fault::Other(_)
        ));
        let busy = windows::core::Error::from_hresult(AUDCLNT_E_DEVICE_IN_USE);
        assert_eq!(open_error(&busy), OutputError::Busy);
        let gone = windows::core::Error::from_hresult(AUDCLNT_E_DEVICE_INVALIDATED);
        assert_eq!(open_error(&gone), OutputError::Lost);
    }

    #[test]
    fn a_raw_mix_format_is_read_as_the_driver_gives_it() {
        // An extensible float format, as most mix formats are.
        let ext = WaveFormat::new(32, 32, &SampleType::Float, 48_000, 2, None);
        // SAFETY: a live WaveFormat's own struct.
        let f = unsafe { format_of_raw(ext.as_waveformatex_ref()) }.unwrap();
        assert_eq!(
            (f.rate, f.channels, f.kind, f.valid_bits),
            (48_000, 2, SampleKind::Float, 32)
        );
        // 24 bits in 32, extensible PCM.
        let ext = WaveFormat::new(32, 24, &SampleType::Int, 44_100, 2, None);
        // SAFETY: as above.
        let f = unsafe { format_of_raw(ext.as_waveformatex_ref()) }.unwrap();
        assert_eq!(
            (f.kind, f.container_bits, f.valid_bits),
            (SampleKind::Int, 32, 24)
        );
        // A plain WAVEFORMATEX of 16-bit PCM.
        let plain = WaveFormat::new(16, 16, &SampleType::Int, 48_000, 2, None)
            .to_waveformatex()
            .unwrap();
        // SAFETY: as above.
        let f = unsafe { format_of_raw(plain.as_waveformatex_ref()) }.unwrap();
        assert_eq!(
            (f.kind, f.container_bits, f.valid_bits),
            (SampleKind::Int, 16, 16)
        );
    }
}
