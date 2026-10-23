//! Windows' sound outputs as devices (`docs/design/devices.md`): every render endpoint read from
//! its property store (what it is, its bus, its form factor, the device it belongs to), the
//! default output, and a watcher that tells the engine when any of it changes.
//!
//! `WasapiDevices` owns a thread ("venuta-devices") that holds the enumerator and the
//! notification client: Windows' callbacks only wake it, and it lists the endpoints again into
//! a snapshot the engine reads without waiting, then tells the engine. Nothing here runs on the
//! engine's control thread or touches its COM.

use crate::devices::{Devices, Endpoint, EndpointState, form_factor};
use crate::output_api::Output;
use crate::wasapi_out::{Com, Event, wasapi_output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use windows::Win32::Devices::FunctionDiscovery::{
    PKEY_Device_ContainerId, PKEY_Device_DeviceDesc, PKEY_Device_EnumeratorName,
    PKEY_DeviceInterface_FriendlyName,
};
use windows::Win32::Foundation::{PROPERTYKEY, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    DEVICE_STATE, DEVICE_STATE_ACTIVE, DEVICE_STATE_DISABLED, DEVICE_STATE_NOTPRESENT,
    DEVICE_STATE_UNPLUGGED, DEVICE_STATEMASK_ALL, EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator,
    PKEY_AudioEndpoint_FormFactor, eConsole, eRender,
};
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantClear, PropVariantToGUID, PropVariantToStringAlloc, PropVariantToUInt32,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ};
use windows::Win32::System::Threading::{SetEvent, WaitForSingleObject};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::core::{PCWSTR, implement};

/// Windows sends a burst of notifications for one change: listed once they stop for this long.
const QUIET_MS: u32 = 100;

/// Every render endpoint (any state) and the default render endpoint for the console role,
/// read on this thread (COM is initialised for it while this runs).
pub fn list_endpoints() -> Result<(Vec<Endpoint>, Option<String>), String> {
    let _com = Com::init();
    // SAFETY: COM is initialised on this thread for the call.
    unsafe {
        let en: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        Ok(list_with(&en))
    }
}

/// # Safety
/// COM must be initialised on this thread.
unsafe fn list_with(en: &IMMDeviceEnumerator) -> (Vec<Endpoint>, Option<String>) {
    let mut out = Vec::new();
    // SAFETY: as the caller promised.
    unsafe {
        if let Ok(all) = en.EnumAudioEndpoints(eRender, DEVICE_STATE(DEVICE_STATEMASK_ALL)) {
            let n = all.GetCount().unwrap_or(0);
            for i in 0..n {
                if let Ok(d) = all.Item(i)
                    && let Some(e) = read_endpoint(&d)
                {
                    out.push(e);
                }
            }
        }
        let default = en
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .ok()
            .and_then(|d| device_id(&d));
        (out, default)
    }
}

/// # Safety
/// COM must be initialised on this thread.
unsafe fn device_id(d: &IMMDevice) -> Option<String> {
    // SAFETY: as the caller promised; the string Windows gives is freed once.
    unsafe {
        let p = d.GetId().ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// # Safety
/// COM must be initialised on this thread.
unsafe fn read_endpoint(d: &IMMDevice) -> Option<Endpoint> {
    // SAFETY: as the caller promised.
    unsafe {
        let id = device_id(d)?;
        let state = match d.GetState().ok()? {
            s if s == DEVICE_STATE_ACTIVE => EndpointState::Active,
            s if s == DEVICE_STATE_DISABLED => EndpointState::Disabled,
            s if s == DEVICE_STATE_UNPLUGGED => EndpointState::Unplugged,
            s if s == DEVICE_STATE_NOTPRESENT => EndpointState::NotPresent,
            _ => EndpointState::NotPresent,
        };
        let store = d.OpenPropertyStore(STGM_READ).ok()?;
        Some(Endpoint {
            id,
            description: text(&store, &PKEY_Device_DeviceDesc).unwrap_or_default(),
            adapter: text(&store, &PKEY_DeviceInterface_FriendlyName).unwrap_or_default(),
            enumerator: text(&store, &PKEY_Device_EnumeratorName).unwrap_or_default(),
            form_factor: number(&store, &PKEY_AudioEndpoint_FormFactor)
                .unwrap_or(form_factor::UNKNOWN),
            container: guid(&store, &PKEY_Device_ContainerId),
            state,
        })
    }
}

/// A string property.
///
/// # Safety
/// COM must be initialised on this thread.
unsafe fn text(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
    // SAFETY: the value is read while it lives, its string freed once, the value cleared once.
    unsafe {
        let mut v = store.GetValue(key).ok()?;
        let s = PropVariantToStringAlloc(&v).ok().and_then(|p| {
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        });
        let _ = PropVariantClear(&mut v);
        s.filter(|s| !s.is_empty())
    }
}

/// A number property.
///
/// # Safety
/// COM must be initialised on this thread.
unsafe fn number(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<u32> {
    // SAFETY: as `text`.
    unsafe {
        let mut v = store.GetValue(key).ok()?;
        let n = PropVariantToUInt32(&v).ok();
        let _ = PropVariantClear(&mut v);
        n
    }
}

/// A GUID property, braced, lower case.
///
/// # Safety
/// COM must be initialised on this thread.
unsafe fn guid(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
    // SAFETY: as `text`.
    unsafe {
        let mut v = store.GetValue(key).ok()?;
        let g = PropVariantToGUID(&v).ok();
        let _ = PropVariantClear(&mut v);
        g.map(|g| format!("{{{g:?}}}").to_lowercase())
    }
}

/// What the devices thread and the engine share.
struct Shared {
    snapshot: Mutex<(Vec<Endpoint>, Option<String>)>,
    watcher: Mutex<Option<Sender<()>>>,
    quit: AtomicBool,
    wake: Event,
}

/// Windows' sound outputs, watched (see the module's notes).
pub struct WasapiDevices {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl WasapiDevices {
    /// Lists the endpoints now, and watches them from here on.
    pub fn new() -> Result<Arc<WasapiDevices>, String> {
        let first = list_endpoints()?;
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(first),
            watcher: Mutex::new(None),
            quit: AtomicBool::new(false),
            wake: Event::new().map_err(|e| e.to_string())?,
        });
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("venuta-devices".into())
            .spawn(move || watch_thread(&s))
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(WasapiDevices {
            shared,
            thread: Some(thread),
        }))
    }
}

impl Drop for WasapiDevices {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::Release);
        // SAFETY: a valid event handle, alive while `shared` is.
        let _ = unsafe { SetEvent(self.shared.wake.0) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Devices for WasapiDevices {
    fn endpoints(&self) -> Vec<Endpoint> {
        self.shared
            .snapshot
            .lock()
            .map(|s| s.0.clone())
            .unwrap_or_default()
    }

    fn default_id(&self) -> Option<String> {
        self.shared.snapshot.lock().ok().and_then(|s| s.1.clone())
    }

    fn open(&self, id: &str, exclusive: bool) -> Box<dyn Output> {
        Box::new(wasapi_output(Some(id), exclusive))
    }

    fn watch(&self) -> Receiver<()> {
        let (tx, rx) = channel();
        if let Ok(mut w) = self.shared.watcher.lock() {
            *w = Some(tx);
        }
        rx
    }
}

/// The notification client: any change wakes the devices thread (a property change is not one:
/// a format change reaches the stream itself, which Windows invalidates).
#[implement(IMMNotificationClient)]
struct Watcher {
    shared: Arc<Shared>,
}

impl Watcher {
    fn poke(&self) {
        // SAFETY: a valid event handle, alive while `shared` is.
        let _ = unsafe { SetEvent(self.shared.wake.0) };
    }
}

impl IMMNotificationClient_Impl for Watcher_Impl {
    fn OnDeviceStateChanged(
        &self,
        _id: &PCWSTR,
        _state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        self.poke();
        Ok(())
    }

    fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        self.poke();
        Ok(())
    }

    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        self.poke();
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _id: &PCWSTR,
    ) -> windows::core::Result<()> {
        if flow == eRender && role == eConsole {
            self.poke();
        }
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _id: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

fn watch_thread(shared: &Arc<Shared>) {
    let _com = Com::init();
    // SAFETY: COM is initialised on this thread; the client is unregistered before it goes.
    unsafe {
        let Ok(en): windows::core::Result<IMMDeviceEnumerator> =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return;
        };
        let client: IMMNotificationClient = Watcher {
            shared: shared.clone(),
        }
        .into();
        let registered = en.RegisterEndpointNotificationCallback(&client).is_ok();
        loop {
            let _ = WaitForSingleObject(shared.wake.0, u32::MAX);
            if shared.quit.load(Ordering::Acquire) {
                break;
            }
            // The rest of the burst.
            while WaitForSingleObject(shared.wake.0, QUIET_MS) == WAIT_OBJECT_0 {
                if shared.quit.load(Ordering::Acquire) {
                    break;
                }
            }
            if shared.quit.load(Ordering::Acquire) {
                break;
            }
            let now = list_with(&en);
            if let Ok(mut s) = shared.snapshot.lock() {
                *s = now;
            }
            if let Ok(w) = shared.watcher.lock()
                && let Some(tx) = w.as_ref()
            {
                let _ = tx.send(());
            }
        }
        if registered {
            let _ = en.UnregisterEndpointNotificationCallback(&client);
        }
    }
}
