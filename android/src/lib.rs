use std::os::fd::RawFd;
use std::sync::{Arc, Mutex, OnceLock};

use jni::objects::{JClass, JString};
use jni::sys::{jint, jstring};
use jni::JNIEnv;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, Notify};

use ssh2proxy_core::{Proxy, ProxyConfig};

static RT: OnceLock<Runtime> = OnceLock::new();
static STOP: Mutex<Option<Arc<Notify>>> = Mutex::new(None);
static PROXY_DNS: Mutex<Option<String>> = Mutex::new(None);

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
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut proxy = Proxy::new(config, tx);
    match rt.block_on(proxy.connect()) {
        Ok(()) => {
            let stop = proxy.stop_handle();
            rt.spawn(async move { let _ = proxy.run_reconnect_loop().await; });
            let mut slot = STOP.lock().unwrap_or_else(|e| e.into_inner());
            *slot = Some(stop);
            0
        }
        Err(_) => -2,
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
        if let Err(e) = dp.run().await {
            log::error!("dataplane exited: {e}");
        }
    });
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
