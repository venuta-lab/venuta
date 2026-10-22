//! Recording on Windows: the loopback of an output (what Windows' mixer sends to it, shared
//! mode streams only) or an input (a line in, a USB interface, a microphone), as 32-bit float
//! at the endpoint's own mix format, with the performance-counter time of every packet.
//!
//! Loopback sends no data while nothing plays; those stretches are filled with silence (by
//! their timestamps) and listed, so the file keeps real time.

use super::timeline::Timeline;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use wasapi::{Direction, SampleType, StreamMode, WaveFormat};

#[derive(Debug, Clone)]
pub enum Source {
    /// The loopback of an output (`None`: the default output).
    Loopback(Option<String>),
    /// An input (`None`: the default input).
    Input(Option<String>),
}

/// The performance counter now, in 100 ns units (the unit WASAPI stamps positions with).
pub fn qpc_100ns() -> u64 {
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let (mut count, mut freq) = (0i64, 0i64);
    // SAFETY: plain calls with valid out pointers.
    unsafe {
        let _ = QueryPerformanceCounter(&mut count);
        let _ = QueryPerformanceFrequency(&mut freq);
    }
    if freq <= 0 {
        return 0;
    }
    (u128::from(count as u64) * 10_000_000 / u128::from(freq as u64)) as u64
}

pub fn unix_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or_default()
}

/// Endpoints, by direction: (id, name, default).
pub fn endpoints(dir: Direction) -> Vec<(String, String, bool)> {
    let _ = wasapi::initialize_mta();
    let default_id = wasapi::get_default_device(&dir)
        .and_then(|d| d.get_id())
        .unwrap_or_default();
    let Ok(col) = wasapi::DeviceCollection::new(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for d in &col {
        let Ok(d) = d else { continue };
        let (Ok(id), Ok(name)) = (d.get_id(), d.get_friendlyname()) else {
            continue;
        };
        out.push((id.clone(), name, id == default_id));
    }
    out
}

pub fn find(dir: Direction, id: Option<&str>) -> Result<wasapi::Device, String> {
    if let Some(id) = id {
        let col = wasapi::DeviceCollection::new(&dir).map_err(|e| e.to_string())?;
        for d in &col {
            if let Ok(d) = d
                && (d.get_id().ok().as_deref() == Some(id)
                    || d.get_friendlyname().ok().as_deref() == Some(id))
            {
                return Ok(d);
            }
        }
        return Err(format!("No endpoint {id:?} (see the devices command)."));
    }
    wasapi::get_default_device(&dir).map_err(|e| format!("No default endpoint ({e})."))
}

/// The mix format of an output: (rate, channels).
pub fn mix_format(id: Option<&str>) -> Result<(u32, u16), String> {
    let _ = wasapi::initialize_mta();
    let d = find(Direction::Render, id)?;
    let c = d.get_iaudioclient().map_err(|e| e.to_string())?;
    let f = c.get_mixformat().map_err(|e| e.to_string())?;
    Ok((f.get_samplespersec(), f.get_nchannels()))
}

/// Where captured samples go (interleaved, on the capture thread).
pub type Sink = Box<dyn FnMut(&[f32]) + Send>;

pub struct Recorder {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<Result<Timeline, String>>,
    pub rate: u32,
    pub channels: u16,
    pub name: String,
}

impl Recorder {
    /// Start recording; `sink` gets the interleaved samples as they come (silence for idle
    /// stretches included), on the recording thread.
    pub fn start(source: Source, sink: Sink) -> Result<Recorder, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let s = stop.clone();
        let handle = std::thread::Builder::new()
            .name("kit-capture".into())
            .spawn(move || run(source, sink, s, ready_tx))
            .map_err(|e| e.to_string())?;
        match ready_rx.recv() {
            Ok(Ok((rate, channels, name))) => Ok(Recorder {
                stop,
                handle,
                rate,
                channels,
                name,
            }),
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => Err(handle
                .join()
                .ok()
                .and_then(|r| r.err())
                .unwrap_or_else(|| "The capture thread ended.".into())),
        }
    }

    pub fn stop(self) -> Result<Timeline, String> {
        self.stop.store(true, Ordering::SeqCst);
        self.handle
            .join()
            .map_err(|_| "The capture thread panicked.".to_string())?
    }
}

type Ready = Result<(u32, u16, String), String>;

fn run(
    source: Source,
    mut sink: Sink,
    stop: Arc<AtomicBool>,
    ready: mpsc::Sender<Ready>,
) -> Result<Timeline, String> {
    let fail = |ready: &mpsc::Sender<Ready>, e: String| {
        let _ = ready.send(Err(e.clone()));
        Err(e)
    };
    let _ = wasapi::initialize_mta();
    let (device, what) = match &source {
        Source::Loopback(id) => (find(Direction::Render, id.as_deref()), "loopback of"),
        Source::Input(id) => (find(Direction::Capture, id.as_deref()), "input"),
    };
    let device = match device {
        Ok(d) => d,
        Err(e) => return fail(&ready, e),
    };
    let name = device.get_friendlyname().unwrap_or_default();
    let mut client = match device.get_iaudioclient() {
        Ok(c) => c,
        Err(e) => return fail(&ready, e.to_string()),
    };
    let mix = match client.get_mixformat() {
        Ok(f) => f,
        Err(e) => return fail(&ready, e.to_string()),
    };
    let (rate, channels) = (mix.get_samplespersec(), mix.get_nchannels());
    let want = WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        rate as usize,
        usize::from(channels),
        None,
    );
    let mode = StreamMode::PollingShared {
        autoconvert: true,
        buffer_duration_hns: 2_000_000,
    };
    if let Err(e) = client.initialize_client(&want, &Direction::Capture, &mode) {
        return fail(&ready, format!("The {what} {name} would not open ({e})."));
    }
    let cap = match client.get_audiocaptureclient() {
        Ok(c) => c,
        Err(e) => return fail(&ready, e.to_string()),
    };
    let ch = usize::from(channels);
    let mut tl = Timeline {
        rate,
        channels,
        source: format!("{what} {name}"),
        ..Timeline::default()
    };
    if let Err(e) = client.start_stream() {
        return fail(&ready, e.to_string());
    }
    tl.start_qpc = qpc_100ns();
    tl.start_unix_ms = unix_ms();
    let _ = ready.send(Ok((rate, channels, name)));
    let mut buf = vec![0u8; 4 * ch * rate as usize];
    let mut samples: Vec<f32> = Vec::with_capacity(ch * rate as usize);
    let mut written: u64 = 0;
    // The time the next frame is due (100 ns units), once data has come.
    let mut next_due: Option<f64> = None;
    let mut last_anchor: Option<u64> = None;
    let per_frame = 1e7 / f64::from(rate);
    let mut first = true;
    let silence = vec![0f32; ch * 4800];
    // KIT_DEBUG_CAPTURE=N prints the first N packets.
    let mut debug: u32 = std::env::var("KIT_DEBUG_CAPTURE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(3));
        loop {
            let size = match cap.get_next_packet_size() {
                Ok(Some(n)) => n,
                Ok(None) => 0,
                Err(e) => {
                    client.stop_stream().ok();
                    return Err(format!("The capture stopped ({e})."));
                }
            };
            if size == 0 {
                break;
            }
            let need = size as usize * 4 * ch;
            if buf.len() < need {
                buf.resize(need, 0);
            }
            let (n, info) = match cap.read_from_device(&mut buf[..need]) {
                Ok(r) => r,
                Err(e) => {
                    client.stop_stream().ok();
                    return Err(format!("The capture stopped ({e})."));
                }
            };
            let n = n as usize;
            if n == 0 {
                continue;
            }
            let ts = info.timestamp;
            if debug > 0 {
                debug -= 1;
                eprintln!(
                    "capture packet: {n} frames, device position {}, stamped {:.1} ms before it was read, silent {}, discontinuity {}",
                    info.index,
                    (qpc_100ns() as f64 - ts as f64) / 1e4,
                    info.flags.silent,
                    info.flags.data_discontinuity
                );
            }
            // Nothing came for a while: the output was idle. Silence, as long as it was.
            if let Some(due) = next_due {
                let late = ts as f64 - due;
                if late > 200_000.0 {
                    let mut fill = (late / per_frame).round() as u64;
                    tl.idle.push((written, fill));
                    while fill > 0 {
                        let k = (fill as usize).min(silence.len() / ch);
                        sink(&silence[..k * ch]);
                        written += k as u64;
                        fill -= k as u64;
                    }
                }
            }
            if info.flags.data_discontinuity && !first {
                tl.capture_glitches.push(written);
            }
            first = false;
            if last_anchor.is_none_or(|a| written >= a + u64::from(rate)) || tl.anchors.is_empty() {
                tl.anchors.push((written, ts));
                last_anchor = Some(written);
            }
            samples.clear();
            if info.flags.silent {
                samples.resize(n * ch, 0.0);
            } else {
                samples.extend(
                    buf[..n * 4 * ch]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b)),
                );
            }
            sink(&samples);
            written += n as u64;
            next_due = Some(ts as f64 + n as f64 * per_frame);
        }
    }
    client.stop_stream().ok();
    Ok(tl)
}
