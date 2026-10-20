//! Windows sound devices through WASAPI.
//!
//! Exclusive mode gives the device each song's own format (rate, depth, channels), so the
//! samples reach the DAC exactly; when the device can't take that format, shared mode hands
//! the song to Windows' mixer, which converts it. Everything here runs on the player's own
//! thread (COM objects stay where they were made).

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
