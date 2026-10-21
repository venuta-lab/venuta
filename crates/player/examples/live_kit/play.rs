//! Playing the kit's own sounds (test music, chirps, silence) through the app's own WASAPI
//! output code (`player::wasapi_out::WasapiDevice`, the same open, wait, write and
//! `GetPosition` path the player uses), timing every period and keeping the device's reports
//! of which frame is at the speaker.

use super::capture::qpc_100ns;
use player::decode::SourceFormat;
use player::output::{Device, DeviceFormat, Mode, encode};
use player::wasapi_out::{ProAudio, WasapiDevice};

#[derive(Debug, Clone, Default)]
pub struct PlayOpts {
    /// Endpoint id (`None`: the default output).
    pub device: Option<String>,
    pub exclusive: bool,
    /// The rate to open at (shared mode: the mix rate, so Windows converts nothing).
    pub rate: u32,
    /// Busy work per period, in µs, as a stand-in for a renderer's cost.
    pub work_us: u64,
}

/// Makes content: `(out, channels, first_frame)`.
type Fill<'a> = dyn FnMut(&mut [f32], usize, u64) + 'a;

/// One period of the render loop.
#[derive(Debug, Clone, Copy)]
pub struct Period {
    /// When the wait for the device returned (100 ns units).
    pub wake: u64,
    /// When the buffer was handed back.
    pub done: u64,
    pub frames: usize,
    /// Frames still queued in the device at the wake, by its own clock.
    pub queued: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Played {
    pub name: String,
    pub mode: Mode,
    pub format: DeviceFormat,
    /// The device's buffer, frames.
    pub buffer_frames: usize,
    /// The stream frame the content's frame 0 was written at.
    pub content_at: u64,
    /// (performance counter, stream frame at the speaker then), after every period.
    pub pairs: Vec<(u64, f64)>,
    pub periods: Vec<Period>,
}

/// Play `frames` frames of content made by `fill(out, channels, first_frame)` (interleaved
/// float, full scale 1), with a buffer of silence before and after. Runs on this thread,
/// scheduled as pro audio, until done.
pub fn play(
    opts: &PlayOpts,
    frames: usize,
    mut fill: impl FnMut(&mut [f32], usize, u64),
) -> Result<Played, String> {
    let _pro = ProAudio::enter();
    let mut dev = WasapiDevice::new(opts.device.as_deref(), opts.exclusive)?;
    let src = SourceFormat {
        rate: opts.rate,
        channels: 2,
        bits: 24,
        lossless: true,
        codec: "WAV".into(),
    };
    let format = dev.open(&src)?;
    let ch = usize::from(format.channels);
    let buffer_frames = dev.latency_frames();
    let mut f32buf: Vec<f32> = Vec::new();
    let mut ibuf: Vec<i32> = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut written: u64 = 0;
    let mut write = |dev: &mut WasapiDevice,
                     n: usize,
                     content: Option<(&mut Fill, u64)>,
                     written: &mut u64|
     -> Result<(), String> {
        f32buf.clear();
        f32buf.resize(n * ch, 0.0);
        if let Some((f, first)) = content {
            f(&mut f32buf, ch, first);
        }
        ibuf.clear();
        ibuf.extend(
            f32buf
                .iter()
                .map(|&v| (f64::from(v.clamp(-1.0, 1.0)) * 2_147_483_647.0) as i32),
        );
        encode(&ibuf, &format, 1.0, &mut bytes);
        dev.write(&bytes, n)?;
        *written += n as u64;
        Ok(())
    };
    // The first buffer is silence, written before the start.
    let pre = dev.available()?;
    write(&mut dev, pre, None, &mut written)?;
    dev.start()?;
    let content_at = written;
    let tail = buffer_frames + opts.rate as usize / 5;
    let end = content_at + (frames + tail) as u64;
    let mut pairs = Vec::new();
    let mut periods = Vec::new();
    while written < end {
        let n = dev.wait()?;
        let wake = qpc_100ns();
        let queued = dev.queued_frames();
        if n == 0 {
            continue;
        }
        let n = n.min((end - written) as usize);
        if opts.work_us > 0 {
            let until = wake + opts.work_us * 10;
            while qpc_100ns() < until {
                std::hint::spin_loop();
            }
        }
        let first = written.saturating_sub(content_at);
        let has = written >= content_at && first < frames as u64;
        if has {
            let take = n.min(frames - first as usize);
            // Content, then silence to the end of the period.
            let mut wrap = |out: &mut [f32], c: usize, f: u64| {
                fill(&mut out[..take * c], c, f);
            };
            write(&mut dev, n, Some((&mut wrap, first)), &mut written)?;
        } else {
            write(&mut dev, n, None, &mut written)?;
        }
        let done = qpc_100ns();
        if let Some(q) = dev.queued_frames() {
            pairs.push((qpc_100ns(), written as f64 - q as f64));
        }
        periods.push(Period {
            wake,
            done,
            frames: n,
            queued,
        });
    }
    let name = dev.name();
    let mode = dev.mode();
    dev.stop()?;
    dev.close();
    Ok(Played {
        name,
        mode,
        format,
        buffer_frames,
        content_at,
        pairs,
        periods,
    })
}
