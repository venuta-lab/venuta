//! Throwaway Android spike (docs/android-spike.md, checks 5 to 9): the Rust core behind one
//! UniFFI facade. Each export answers one question on the device; nothing here is the real
//! `venuta-ffi`.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SpikeError {
    #[error("{reason}")]
    Failed { reason: String },
}

fn failed(message: impl Into<String>) -> SpikeError {
    SpikeError::Failed {
        reason: message.into(),
    }
}

// --- logcat and panics ---------------------------------------------------------------------

#[cfg(target_os = "android")]
mod log {
    use std::ffi::{CString, c_char, c_int};
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }
    pub fn write(text: &str) {
        let tag = CString::new("VENUTA_SPIKE_RS").unwrap();
        let text = CString::new(text.replace('\0', " ")).unwrap();
        unsafe {
            __android_log_write(4, tag.as_ptr(), text.as_ptr());
        }
    }
}
#[cfg(not(target_os = "android"))]
mod log {
    pub fn write(text: &str) {
        eprintln!("VENUTA_SPIKE_RS {text}");
    }
}

static PANICS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn hook_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let msg = format!("panic: {info}");
            log::write(&msg);
            if let Ok(mut p) = PANICS.lock() {
                p.push(msg);
            }
        }));
    });
}

/// Panics seen (and caught) since the last call.
#[uniffi::export]
pub fn take_panics() -> Vec<String> {
    PANICS.lock().map(|mut p| std::mem::take(&mut *p)).unwrap_or_default()
}

// --- check 8: load, one call, paths ---------------------------------------------------------

/// What the core is and what the process environment looks like from Rust.
#[uniffi::export]
pub fn core_info() -> String {
    hook_panics();
    let env = |k: &str| std::env::var(k).unwrap_or_else(|_| "<unset>".into());
    format!(
        "engine {} arch {} os {} panic={} | HOME={} TMPDIR={} XDG_DATA_HOME={} APPDATA={} LANG={} | temp_dir={} | user_dir={}",
        engine::VERSION,
        std::env::consts::ARCH,
        std::env::consts::OS,
        if cfg!(panic = "unwind") { "unwind" } else { "abort" },
        env("HOME"),
        env("TMPDIR"),
        env("XDG_DATA_HOME"),
        env("APPDATA"),
        env("LANG"),
        std::env::temp_dir().display(),
        engine::paths::user_dir().display(),
    )
}

/// Stand-in for the missing `paths::init(user, cache, temp)`: point the engine's lookups at the
/// app's own folders, then prove the engine can write there (a file, and its SQLite index).
#[uniffi::export]
pub fn init_paths(files_dir: String, cache_dir: String) -> Result<String, SpikeError> {
    hook_panics();
    // SAFETY: called once at start, before any other thread of the core reads the environment.
    unsafe {
        std::env::set_var("XDG_DATA_HOME", &files_dir);
        std::env::set_var("HOME", &files_dir);
        std::env::set_var("TMPDIR", &cache_dir);
    }
    let user = engine::paths::user_dir();
    let data = engine::paths::app_data_dir();
    std::fs::create_dir_all(&user).map_err(|e| failed(format!("user_dir {}: {e}", user.display())))?;
    std::fs::create_dir_all(&data).map_err(|e| failed(format!("app_data_dir {}: {e}", data.display())))?;
    let probe = data.join("spike-probe.txt");
    std::fs::write(&probe, "venuta").map_err(|e| failed(format!("write {}: {e}", probe.display())))?;
    let back = std::fs::read_to_string(&probe).map_err(|e| failed(e.to_string()))?;
    let db = data.join("index.sqlite");
    let index = engine::Index::open(&db, &files_dir).map_err(|e| failed(format!("Index::open: {e}")))?;
    drop(index);
    let size = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0);
    Ok(format!(
        "user_dir={} app_data_dir={} probe={back} index={} ({size} bytes) temp_dir={}",
        user.display(),
        data.display(),
        db.display(),
        std::env::temp_dir().display()
    ))
}

/// Decode a generated one-second WAV through the player's own decoder (symphonia on device).
#[uniffi::export]
pub fn decode_smoke(cache_dir: String) -> Result<String, SpikeError> {
    let path = PathBuf::from(cache_dir).join("spike-tone.wav");
    let rate = 44_100u32;
    let frames = rate as usize;
    let mut wav = Vec::with_capacity(44 + frames * 4);
    let data_len = (frames * 4) as u32;
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        let s = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 8000.0) as i16;
        wav.extend_from_slice(&s.to_le_bytes());
        wav.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(&path, &wav).map_err(|e| failed(e.to_string()))?;
    let t = Instant::now();
    let mut src = player::decode::Source::open(&path).map_err(failed)?;
    let mut out = Vec::new();
    let mut samples = 0usize;
    loop {
        out.clear();
        match src.next(&mut out).map_err(failed)? {
            player::decode::DecodeState::End => break,
            _ => samples += out.len(),
        }
    }
    Ok(format!(
        "decoded {samples} samples in {} ms, duration_ms={:?}",
        t.elapsed().as_millis(),
        src.duration_ms()
    ))
}

/// UniFFI async on tokio: returns after `ms`.
#[uniffi::export(async_runtime = "tokio")]
pub async fn async_echo(ms: u64) -> String {
    let t = Instant::now();
    tokio::time::sleep(Duration::from_millis(ms)).await;
    format!("slept {} ms on tokio", t.elapsed().as_millis())
}

/// Keeps the rest of the core reachable so the .so size is honest: never called with `true`.
#[uniffi::export]
pub fn link_everything(really: bool) -> String {
    if !really {
        return "not run".into();
    }
    let mut index = match engine::Index::open_in_memory("/") {
        Ok(i) => i,
        Err(e) => return e.to_string(),
    };
    let _ = engine::scan(&mut index, std::path::Path::new("/"));
    let p = player::engine::Player::<PathBuf>::spawn(
        player::engine::OutputChoice::Device {
            id: None,
            exclusive: false,
        },
        |_| {},
    );
    p.send(player::engine::Cmd::Quit);
    let live = media_access::LiveCatalogAccess::new().is_ok();
    format!("live={live}")
}

// --- check 9: keyring 3 and iroh ------------------------------------------------------------

/// Save through the engine's SystemVault (keyring 3) and read it straight back.
#[uniffi::export]
pub fn vault_put_get(key: String, value: String) -> String {
    use engine::accounts::Vault;
    let v = engine::accounts::SystemVault;
    let put = v.put(&key, &value);
    let get = v.get(&key);
    format!("put={put:?} get_same_process={get:?}")
}

#[uniffi::export]
pub fn vault_get(key: String) -> String {
    use engine::accounts::Vault;
    format!("get={:?}", engine::accounts::SystemVault.get(&key))
}

/// The DNS lookup and an N0 endpoint, the way Listen together starts one.
#[uniffi::export(async_runtime = "tokio")]
pub async fn iroh_probe(label: String) -> String {
    hook_panics();
    let t = Instant::now();
    let resolver = iroh::dns::DnsResolver::new();
    let dns = match resolver
        .lookup_ipv4("dns.iroh.link", Duration::from_secs(5))
        .await
    {
        Ok(a) => format!("ok {:?}", a.collect::<Vec<_>>()),
        Err(e) => format!("err {e}"),
    };
    let dns_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let ep = match iroh::Endpoint::builder(iroh::endpoint::presets::N0).bind().await {
        Ok(ep) => ep,
        Err(e) => return format!("[{label}] dns={dns} ({dns_ms} ms) bind err {e}"),
    };
    let bind_ms = t.elapsed().as_millis();
    let online = tokio::time::timeout(Duration::from_secs(10), ep.online())
        .await
        .is_ok();
    let online_ms = t.elapsed().as_millis();
    let addr = format!("{:?}", ep.addr());
    ep.close().await;
    let addr: String = addr.chars().take(300).collect();
    let panics = take_panics();
    format!(
        "[{label}] dns={dns} ({dns_ms} ms) bind {bind_ms} ms online={online} ({online_ms} ms) addr={addr} panics={panics:?}"
    )
}

/// JNI side: `System.loadLibrary` calls this; it keeps the VM for `installContext`.
#[cfg(target_os = "android")]
mod jni_glue {
    use jni_sys::{JNI_VERSION_1_6, JNIEnv, JavaVM, jclass, jint, jobject};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicPtr, Ordering};

    static VM: AtomicPtr<JavaVM> = AtomicPtr::new(std::ptr::null_mut());

    #[unsafe(no_mangle)]
    pub extern "system" fn JNI_OnLoad(vm: *mut JavaVM, _reserved: *mut c_void) -> jint {
        VM.store(vm, Ordering::SeqCst);
        super::log::write("JNI_OnLoad: VM kept");
        JNI_VERSION_1_6
    }

    /// `Native.installContext(applicationContext)`: the fix iroh asks for.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_io_github_venuta_1lab_spike_Native_installContext(
        env: *mut JNIEnv,
        _class: jclass,
        context: jobject,
    ) -> jint {
        let vm = VM.load(Ordering::SeqCst);
        if vm.is_null() {
            return -1;
        }
        unsafe {
            let new_global = (**env).NewGlobalRef.expect("NewGlobalRef");
            let global = new_global(env, context);
            iroh::dns::install_android_jni_context(vm as *mut c_void, global as *mut c_void);
        }
        super::log::write("installContext: iroh JNI context installed");
        0
    }
}
