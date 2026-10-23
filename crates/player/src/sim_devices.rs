//! Simulated sound devices, for tests (`docs/design/devices.md`): a set of endpoints that come
//! and go, a default that moves, a mix format that changes under a stream, the exclusive
//! formats a device takes, another app holding one, sleep and wake. Every output opened on one
//! is a `SimOutput` at the format the endpoint would give (exclusive by the plan from its
//! formats, else its mix format), kept with its endpoint for the test to judge.

use crate::devices::{Devices, Endpoint, EndpointState, FormatTable, ProbeCache, plan_exclusive};
use crate::output::{DeviceFormat, DeviceNotes, Mode};
use crate::output_api::{
    Latency, OpenRequest, Output, OutputError, OutputEvent, OutputInfo, Presentation, Render,
};
use crate::sim::{SimConfig, SimHandle, SimOutput};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use together::time::Timebase;

/// One simulated endpoint.
#[derive(Clone)]
pub struct SimEndpoint {
    pub endpoint: Endpoint,
    /// Shared mode's format (Windows' mix format).
    pub mix: DeviceFormat,
    /// The exclusive formats it takes (`None`: it has no exclusive mode).
    pub exclusive: Option<FormatTable>,
    /// Another app holds it in exclusive mode.
    pub busy: bool,
    /// Every open fails (a driver that never answers).
    pub fails: bool,
    /// Its DAC (period, buffer, skew...); the format is set at each open.
    pub cfg: SimConfig,
}

/// An output the engine opened, in the order opened.
#[derive(Clone)]
pub struct Opening {
    pub id: String,
    pub mode: Mode,
    pub format: DeviceFormat,
    pub why_shared: Option<String>,
    pub handle: SimHandle,
}

#[derive(Default)]
struct Inner {
    eps: Vec<SimEndpoint>,
    default: Option<String>,
    watcher: Option<Sender<()>>,
    opened: Vec<Opening>,
    /// Outputs open now, by endpoint (a serial each, to forget the one closed).
    live: Vec<(String, u64, SimHandle)>,
    next: u64,
}

/// A simulated set of sound devices on a test's clock.
pub struct SimDevices {
    tb: Arc<dyn Timebase>,
    inner: Mutex<Inner>,
    probes: ProbeCache,
    probed: AtomicUsize,
    me: Weak<SimDevices>,
}

impl SimDevices {
    pub fn new(tb: Arc<dyn Timebase>) -> Arc<SimDevices> {
        Arc::new_cyclic(|me| SimDevices {
            tb,
            inner: Mutex::default(),
            probes: ProbeCache::default(),
            probed: AtomicUsize::new(0),
            me: me.clone(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn tell(g: &Inner) {
        if let Some(w) = &g.watcher {
            let _ = w.send(());
        }
    }

    /// The outputs on `id` are lost (render no longer called, `Lost` told).
    fn lose_on(g: &Inner, id: &str) {
        for (on, _, h) in &g.live {
            if on == id {
                h.lose();
            }
        }
    }

    /// A new endpoint (or one replaced by id).
    pub fn add(&self, ep: SimEndpoint) {
        let mut g = self.lock();
        g.eps.retain(|e| e.endpoint.id != ep.endpoint.id);
        g.eps.push(ep);
        Self::tell(&g);
    }

    /// An endpoint plugged in, unplugged, switched off: any other state than active loses the
    /// outputs on it.
    pub fn set_state(&self, id: &str, state: EndpointState) {
        let mut g = self.lock();
        if let Some(e) = g.eps.iter_mut().find(|e| e.endpoint.id == id) {
            e.endpoint.state = state;
        }
        if state != EndpointState::Active {
            Self::lose_on(&g, id);
        }
        Self::tell(&g);
    }

    /// Windows' default output (the console role).
    pub fn set_default(&self, id: Option<&str>) {
        let mut g = self.lock();
        g.default = id.map(str::to_owned);
        Self::tell(&g);
    }

    /// Windows changes an endpoint's mix format: a stream on it is invalidated (lost). No
    /// endpoint change is told (Windows says a property changed).
    pub fn set_mix(&self, id: &str, mix: DeviceFormat) {
        let mut g = self.lock();
        if let Some(e) = g.eps.iter_mut().find(|e| e.endpoint.id == id) {
            e.mix = mix;
        }
        Self::lose_on(&g, id);
    }

    /// Another app takes (or lets go of) an endpoint in exclusive mode.
    pub fn set_busy(&self, id: &str, busy: bool) {
        let mut g = self.lock();
        if let Some(e) = g.eps.iter_mut().find(|e| e.endpoint.id == id) {
            e.busy = busy;
        }
    }

    /// The computer goes to sleep: every stream is gone, and the endpoints `away` (a USB DAC
    /// powered down, Bluetooth disconnected, a monitor) are not present until they come back.
    /// Nothing is told: nothing runs while it sleeps (pass the time with `SimClock::leap`).
    pub fn sleep(&self, away: &[&str]) {
        let mut g = self.lock();
        for (_, _, h) in &g.live {
            h.lose();
        }
        for e in g.eps.iter_mut() {
            if away.contains(&e.endpoint.id.as_str()) {
                e.endpoint.state = EndpointState::NotPresent;
            }
        }
    }

    /// It wakes: the devices are listed again (those away come back with `set_state`).
    pub fn wake(&self) {
        Self::tell(&self.lock());
    }

    /// Every output opened so far, in order.
    pub fn opened(&self) -> Vec<Opening> {
        self.lock().opened.clone()
    }

    /// How many times a device was asked for its exclusive formats.
    pub fn probed(&self) -> usize {
        self.probed.load(Ordering::Relaxed)
    }

    /// Opens a `SimOutput` on `id` for `want`, as the platform would.
    fn open_on(
        &self,
        id: &str,
        exclusive: bool,
        want: &OpenRequest,
    ) -> Result<(SimOutput, OutputInfo, Mode, DeviceNotes, u64), OutputError> {
        let ep = {
            let g = self.lock();
            g.eps
                .iter()
                .find(|e| e.endpoint.id == id && e.endpoint.state == EndpointState::Active)
                .cloned()
        }
        .ok_or(OutputError::Lost)?;
        if ep.fails {
            return Err(OutputError::Other(
                "The sound device did not answer.".into(),
            ));
        }
        let mut notes = DeviceNotes {
            endpoint_id: Some(id.to_owned()),
            ..DeviceNotes::default()
        };
        let mut chosen: Option<DeviceFormat> = None;
        if exclusive {
            if ep.busy {
                notes.why_shared = Some("another app is using it in exclusive mode".into());
            } else {
                let table = self.probes.get_or_probe(id, want.channels, || {
                    self.probed.fetch_add(1, Ordering::Relaxed);
                    Some(ep.exclusive.clone().unwrap_or_default())
                });
                if let Some(t) = &table {
                    chosen = plan_exclusive(t, want).into_iter().next();
                }
                if chosen.is_none() {
                    notes.why_shared = Some("it takes no exclusive format for this song".into());
                }
                notes.exclusive_formats = table;
            }
        }
        let (format, mode) = match chosen {
            Some(f) => (f, Mode::Exclusive),
            None => (ep.mix.clone(), Mode::Shared),
        };
        let mut cfg = ep.cfg.clone();
        cfg.format = Some(format.clone());
        let (mut out, handle) = SimOutput::new(self.tb.clone(), cfg);
        let info = out.open(want)?;
        let mut g = self.lock();
        g.next += 1;
        let serial = g.next;
        g.live.push((id.to_owned(), serial, handle.clone()));
        g.opened.push(Opening {
            id: id.to_owned(),
            mode,
            format,
            why_shared: notes.why_shared.clone(),
            handle,
        });
        Ok((out, info, mode, notes, serial))
    }

    fn closed(&self, serial: u64) {
        self.lock().live.retain(|(_, s, _)| *s != serial);
    }
}

impl Devices for SimDevices {
    fn endpoints(&self) -> Vec<Endpoint> {
        self.lock().eps.iter().map(|e| e.endpoint.clone()).collect()
    }

    fn default_id(&self) -> Option<String> {
        self.lock().default.clone()
    }

    fn open(&self, id: &str, exclusive: bool) -> Box<dyn Output> {
        Box::new(SimDeviceOutput {
            devices: self.me.clone(),
            id: id.to_owned(),
            exclusive,
            out: None,
            mode: if exclusive {
                Mode::Exclusive
            } else {
                Mode::Shared
            },
            notes: None,
            serial: 0,
            presentation: Arc::new(Presentation::default()),
        })
    }

    fn watch(&self) -> Receiver<()> {
        let (tx, rx) = channel();
        self.lock().watcher = Some(tx);
        rx
    }
}

/// An output on a simulated endpoint: a `SimOutput` once open.
struct SimDeviceOutput {
    devices: Weak<SimDevices>,
    id: String,
    exclusive: bool,
    out: Option<SimOutput>,
    mode: Mode,
    notes: Option<DeviceNotes>,
    serial: u64,
    presentation: Arc<Presentation>,
}

impl Output for SimDeviceOutput {
    fn name(&self) -> String {
        self.id.clone()
    }

    fn mode(&self) -> Mode {
        self.mode
    }

    fn open(&mut self, want: &OpenRequest) -> Result<OutputInfo, OutputError> {
        if self.out.is_some() {
            return Err(OutputError::State("already open".into()));
        }
        let d = self.devices.upgrade().ok_or(OutputError::Lost)?;
        let (out, info, mode, notes, serial) = d.open_on(&self.id, self.exclusive, want)?;
        self.presentation = out.presentation();
        self.out = Some(out);
        self.mode = mode;
        self.notes = Some(notes);
        self.serial = serial;
        Ok(info)
    }

    fn start(&mut self, render: Box<dyn Render>) -> Result<(), OutputError> {
        match self.out.as_mut() {
            Some(o) => o.start(render),
            None => Err(OutputError::State("start before open".into())),
        }
    }

    fn stop(&mut self) -> Option<Box<dyn Render>> {
        self.out.as_mut()?.stop()
    }

    fn close(&mut self) {
        if let Some(mut o) = self.out.take() {
            o.close();
            if let Some(d) = self.devices.upgrade() {
                d.closed(self.serial);
            }
        }
    }

    fn presentation(&self) -> Arc<Presentation> {
        self.presentation.clone()
    }

    fn events(&self) -> Receiver<OutputEvent> {
        match &self.out {
            Some(o) => o.events(),
            None => channel().1,
        }
    }

    fn latency(&self) -> Option<Latency> {
        self.out.as_ref()?.latency()
    }

    fn notes(&self) -> Option<DeviceNotes> {
        self.notes.clone()
    }
}

impl Drop for SimDeviceOutput {
    fn drop(&mut self) {
        self.close();
    }
}
