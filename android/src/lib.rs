use std::collections::VecDeque;
use std::os::fd::RawFd;
#[cfg(target_os = "android")]
use std::os::raw::c_char;
use std::sync::{Arc, Mutex, OnceLock};

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jstring};
use jni::JNIEnv;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, Notify};

use ssh2proxy_core::stats::TrafficStats;
use ssh2proxy_core::udp::{UdpRelayManager, UdpResponse};
use ssh2proxy_core::{Proxy, ProxyConfig, ProxyState, StateEvent};

static RT: OnceLock<Runtime> = OnceLock::new();
static STOP: Mutex<Option<Arc<Notify>>> = Mutex::new(None);
static TUN_STOP: Mutex<Option<Arc<Notify>>> = Mutex::new(None);
static PROXY_DNS: Mutex<Option<String>> = Mutex::new(None);
static SOCKS_ADDR: Mutex<Option<String>> = Mutex::new(None);
static ACTIVE_SSH: Mutex<Option<ssh2proxy_core::SharedSsh>> = Mutex::new(None);
static UDP_MGR: Mutex<Option<Arc<UdpRelayManager>>> = Mutex::new(None);
static UDP_RESPONSE: Mutex<Option<mpsc::UnboundedReceiver<UdpResponse>>> = Mutex::new(None);
static STATS: Mutex<Option<Arc<TrafficStats>>> = Mutex::new(None);
static EVENTS: Mutex<VecDeque<(u64, String)>> = Mutex::new(VecDeque::new());
/// 0=Error 1=Warn 2=Info 3=Debug
static LOG_LEVEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn push_event(line: String) {
    let mut q = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
    q.push_back((now_millis(), line));
    while q.len() > 500 {
        q.pop_front();
    }
}

fn level_u8(l: log::Level) -> u8 {
    match l {
        log::Level::Error => 0,
        log::Level::Warn => 1,
        log::Level::Info => 2,
        log::Level::Debug => 3,
        log::Level::Trace => 4,
    }
}

struct CombinedLogger;
static LOGGER: CombinedLogger = CombinedLogger;

impl log::Log for CombinedLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        level_u8(metadata.level()) <= LOG_LEVEL.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = record.args().to_string();
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
pub extern "system" fn Java_com_wuko233_ssh2proxy_NativeBridge_setLogLevel(
    _env: JNIEnv,
    _class: JClass,
    debug: jboolean,
) {
    let level = if debug != 0 { 3u8 } else { 2u8 };
    LOG_LEVEL.store(level, std::sync::atomic::Ordering::Relaxed);
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
    {
        let mut slot = SOCKS_ADDR.lock().unwrap_or_else(|e| e.into_inner());
        *slot = Some(format!("127.0.0.1:{}", config.socks_port));
    }
    let rt = match runtime() {
        Some(rt) => rt,
        None => return -2,
    };
    log::info!("Connecting to {}:{}", config.host, config.port);
    log::info!("DNS: {}", config.dns_server);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let udp = if config.udp_enabled {
        let (mgr, response_rx) = UdpRelayManager::new();
        {
            let mut slot = UDP_MGR.lock().unwrap_or_else(|e| e.into_inner());
            *slot = Some(mgr.clone());
        }
        {
            let mut slot = UDP_RESPONSE.lock().unwrap_or_else(|e| e.into_inner());
            *slot = Some(response_rx);
        }
        Some(mgr)
    } else {
        {
            let mut slot = UDP_MGR.lock().unwrap_or_else(|e| e.into_inner());
            *slot = None;
        }
        {
            let mut slot = UDP_RESPONSE.lock().unwrap_or_else(|e| e.into_inner());
            *slot = None;
        }
        None
    };
    {
        // 每次连接重置流量统计
        let mut slot = STATS.lock().unwrap_or_else(|e| e.into_inner());
        *slot = Some(Arc::new(TrafficStats::default()));
    }
    log::info!("UDP: {}", if config.udp_enabled { "enabled" } else { "disabled" });
    let bind_addr = config.bind_addr.clone();
    let socks_port = config.socks_port;
    let http_port = config.http_port;
    let mut proxy = Proxy::new(config, tx, udp);
    match rt.block_on(proxy.connect()) {
        Ok(()) => {
            let stop = proxy.stop_handle();
            {
                let mut slot = ACTIVE_SSH.lock().unwrap_or_else(|e| e.into_inner());
                *slot = Some(proxy.ssh_shared());
            }
            rt.spawn(async move {
                while let Some(ev) = rx.recv().await {
                    match ev {
                        StateEvent::StateChanged(s) => {
                            let msg = match s {
                                ProxyState::Disconnected => "Disconnected",
                                ProxyState::Connecting => "Connecting",
                                ProxyState::Connected => "Connected",
                                ProxyState::Reconnecting => "Reconnecting",
                                ProxyState::Error => "Error",
                            };
                            log::info!("{msg}");
                        }
                        StateEvent::Error(e) => log::error!("{e}"),
                        StateEvent::Log(l) => log::info!("{l}"),
                    }
                }
            });
            rt.spawn(async move { let _ = proxy.run_reconnect_loop().await; });
            let mut slot = STOP.lock().unwrap_or_else(|e| e.into_inner());
            *slot = Some(stop);
            log::info!("Socks5 server on {}:{}", bind_addr, socks_port);
            log::info!("HTTP proxy on {}:{}", bind_addr, http_port);
            0
        }
        Err(e) => {
            log::error!("Connect failed: {e}");
            -2
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_wuko233_ssh2proxy_NativeBridge_runLatencyTest(
    mut env: JNIEnv,
    _class: JClass,
    target: JString,
) -> jstring {
    let target: String = env.get_string(&target).map(|s| s.into()).unwrap_or_default();
    let (target_host, target_port) = parse_target(&target);
    let ssh = current_ssh();
    let socks_addr = SOCKS_ADDR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| "127.0.0.1:1080".to_string());
    let result = match (runtime(), ssh) {
        (Some(rt), Some(ssh)) => rt.block_on(ssh2proxy_core::probe::measure_latency(
            &ssh,
            &ssh2proxy_core::socks5::Socks5Dialer { addr: socks_addr.parse().unwrap() },
            &target_host,
            target_port,
        )),
        (_, None) => ssh2proxy_core::probe::LatencyResult {
            ssh_ok: false,
            ssh_latency_ms: None,
            ssh_error: Some("SSH unavailable".into()),
            proxy_ok: false,
            proxy_latency_ms: None,
            proxy_error: Some("SSH unavailable".into()),
        },
        (None, Some(_)) => ssh2proxy_core::probe::LatencyResult {
            ssh_ok: false,
            ssh_latency_ms: None,
            ssh_error: Some("runtime unavailable".into()),
            proxy_ok: false,
            proxy_latency_ms: None,
            proxy_error: Some("runtime unavailable".into()),
        },
    };
    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
    match env.new_string(json) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
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
            addr: SOCKS_ADDR
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .unwrap_or_else(|| "127.0.0.1:1080".to_string())
                .parse()
                .unwrap(),
        };
        let udp = UDP_MGR
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let udp_rx = UDP_RESPONSE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let stats = STATS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_default();
        let mut dp = ssh2proxy_core::dataplane::DataPlane::new(device, socks, dns, udp, udp_rx, stats);
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
    let stats = STATS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default();
    let (up, down) = stats.snapshot();
    let json = format!("{{\"up\":{up},\"down\":{down}}}");
    match env.new_string(json) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_wuko233_ssh2proxy_NativeBridge_pollEvents(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let events: Vec<(u64, String)> = {
        let mut q = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
        q.drain(..).collect()
    };
    let json = serde_json::to_string(&events).unwrap_or_else(|_| "[]".to_string());
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
        addr: SOCKS_ADDR
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_else(|| "127.0.0.1:1080".to_string())
            .parse()
            .unwrap(),
    };
    let result = rt.block_on(ssh2proxy_core::probe::connectivity_test(&dialer, &dns, &domain));
    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
    log::info!("connectivity test result: {json}");
    match env.new_string(json) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// 解析 "host:port" 形式的目标；默认 "223.5.5.5:53"，无端口时默认 443。
fn parse_target(target: &str) -> (String, u16) {
    if target.is_empty() {
        return ("223.5.5.5".to_string(), 53);
    }
    if let Some(pos) = target.rfind(':') {
        if let Ok(port) = target[pos + 1..].parse::<u16>() {
            let host = &target[..pos];
            if !host.is_empty() {
                return (host.to_string(), port);
            }
        }
    }
    (target.to_string(), 443)
}

/// 读取当前 SSH 会话（重连后由 Proxy 内部更新共享单元）。
fn current_ssh() -> Option<Arc<ssh2proxy_core::ssh::SshClient>> {
    ACTIVE_SSH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .and_then(|cell| cell.lock().unwrap_or_else(|e| e.into_inner()).clone())
}
