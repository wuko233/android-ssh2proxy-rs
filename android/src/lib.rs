use std::collections::VecDeque;
use std::os::fd::RawFd;
#[cfg(target_os = "android")]
use std::os::raw::c_char;
use std::sync::{Arc, Mutex, OnceLock};

use jni::objects::{JClass, JString};
use jni::sys::{jint, jstring};
use jni::JNIEnv;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, Notify};

use ssh2proxy_core::{Proxy, ProxyConfig, StateEvent};

static RT: OnceLock<Runtime> = OnceLock::new();
static STOP: Mutex<Option<Arc<Notify>>> = Mutex::new(None);
static TUN_STOP: Mutex<Option<Arc<Notify>>> = Mutex::new(None);
static PROXY_DNS: Mutex<Option<String>> = Mutex::new(None);
static EVENTS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

fn push_event(line: String) {
    let mut q = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
    q.push_back(line);
    while q.len() > 500 {
        q.pop_front();
    }
}

struct CombinedLogger;
static LOGGER: CombinedLogger = CombinedLogger;

impl log::Log for CombinedLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("[{}] {}", record.level(), record.args());
        push_event(line.clone());
        #[cfg(target_os = "android")]
        {
            let prio = match record.level() {
                log::Level::Error => android_log_sys::LogPriority::ERROR as i32,
                log::Level::Warn => android_log_sys::LogPriority::WARN as i32,
                log::Level::Info => android_log_sys::LogPriority::INFO as i32,
                log::Level::Debug => android_log_sys::LogPriority::DEBUG as i32,
                log::Level::Trace => android_log_sys::LogPriority::VERBOSE as i32,
            };
            let tag = b"ssh2proxy\0";
            if let Ok(c) = std::ffi::CString::new(line) {
                unsafe {
                    android_log_sys::__android_log_write(prio, tag.as_ptr() as *const c_char, c.as_ptr());
                }
            }
        }
        #[cfg(not(target_os = "android"))]
        eprintln!("{line}");
    }

    fn flush(&self) {}
}

fn runtime() -> Option<&'static Runtime> {
    RT.get().or_else(|| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .ok()?;
        let _ = RT.set(rt);
        RT.get()
    })
}

#[no_mangle]
pub extern "system" fn JNI_OnLoad(_vm: jni::JavaVM, _reserved: *mut std::ffi::c_void) -> jint {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Debug);
    jni::sys::JNI_VERSION_1_6
}

#[no_mangle]
pub extern "system" fn Java_com_wuko233_ssh2proxy_NativeBridge_connect(
    mut env: JNIEnv,
    _class: JClass,
    config_json: JString,
) -> jint {
    let json: String = env.get_string(&config_json).map(|s| s.into()).unwrap_or_default();
    let config: ProxyConfig = match serde_json::from_str(&json) {
        Ok(c) => c,
        Err(_) => return -1,
    };
    {
        let mut dns = PROXY_DNS.lock().unwrap_or_else(|e| e.into_inner());
        *dns = Some(config.dns_server.clone());
    }
    let rt = match runtime() {
        Some(rt) => rt,
        None => return -2,
    };
    log::info!(
        "connecting to {}:{} as {} (password auth)",
        config.host,
        config.port,
        config.username
    );
    log::info!("dns server: {}", config.dns_server);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut proxy = Proxy::new(config, tx);
    match rt.block_on(proxy.connect()) {
        Ok(()) => {
            let stop = proxy.stop_handle();
            rt.spawn(async move {
                while let Some(ev) = rx.recv().await {
                    match ev {
                        StateEvent::StateChanged(s) => log::info!("state: {:?}", s),
                        StateEvent::Error(e) => log::error!("{e}"),
                        StateEvent::Log(l) => log::info!("{l}"),
                    }
                }
            });
            rt.spawn(async move { let _ = proxy.run_reconnect_loop().await; });
            let mut slot = STOP.lock().unwrap_or_else(|e| e.into_inner());
            *slot = Some(stop);
            log::info!("connected; socks5 listening on 127.0.0.1:1080");
            0
        }
        Err(e) => {
            log::error!("connect failed: {e}");
            -2
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_wuko233_ssh2proxy_NativeBridge_setTunFd(
    _env: JNIEnv,
    _class: JClass,
    fd: jint,
) {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return,
    };
    let raw: RawFd = fd;
    let dns = PROXY_DNS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| "8.8.8.8".to_string());
    rt.spawn(async move {
        let mut cfg = tun::Configuration::default();
        cfg.raw_fd(raw);
        let device = match tun::create_as_async(&cfg) {
            Ok(d) => d,
            Err(e) => {
                log::error!("tun create failed: {e}");
                return;
            }
        };
        let socks = ssh2proxy_core::socks5::Socks5Dialer {
            addr: "127.0.0.1:1080".parse().unwrap(),
        };
        let mut dp = ssh2proxy_core::dataplane::DataPlane::new(device, socks, dns);
        {
            let mut slot = TUN_STOP.lock().unwrap_or_else(|e| e.into_inner());
            *slot = Some(dp.stop_handle());
        }
        if let Err(e) = dp.run().await {
            log::error!("dataplane exited: {e}");
        }
        log::info!("dataplane stopped");
    });
}

#[no_mangle]
pub extern "system" fn Java_com_wuko233_ssh2proxy_NativeBridge_closeTun(
    _env: JNIEnv,
    _class: JClass,
) {
    let stop = TUN_STOP.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(stop) = stop {
        stop.notify_one();
    }
}

#[no_mangle]
pub extern "system" fn Java_com_wuko233_ssh2proxy_NativeBridge_disconnect(
    _env: JNIEnv,
    _class: JClass,
) {
    let stop = STOP.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(stop) = stop {
        stop.notify_one();
    }
}

#[no_mangle]
pub extern "system" fn Java_com_wuko233_ssh2proxy_NativeBridge_getStats(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    match env.new_string("{}") {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_wuko233_ssh2proxy_NativeBridge_pollEvents(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let lines: Vec<String> = {
        let mut q = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
        q.drain(..).collect()
    };
    let json = serde_json::to_string(&lines).unwrap_or_else(|_| "[]".to_string());
    match env.new_string(json) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_wuko233_ssh2proxy_NativeBridge_runConnectivityTest(
    mut env: JNIEnv,
    _class: JClass,
    domain: JString,
) -> jstring {
    let domain: String = env
        .get_string(&domain)
        .map(|s| s.into())
        .unwrap_or_default();
    let domain = if domain.is_empty() { "www.baidu.com".to_string() } else { domain };
    let rt = match runtime() {
        Some(rt) => rt,
        None => return std::ptr::null_mut(),
    };
    let dns = PROXY_DNS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| "8.8.8.8".to_string());
    let dialer = ssh2proxy_core::socks5::Socks5Dialer {
        addr: "127.0.0.1:1080".parse().unwrap(),
    };
    let result = rt.block_on(ssh2proxy_core::probe::connectivity_test(&dialer, &dns, &domain));
    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
    log::info!("connectivity test result: {json}");
    match env.new_string(json) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
