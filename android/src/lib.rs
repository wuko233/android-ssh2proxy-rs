use std::os::fd::RawFd;
use std::sync::OnceLock;

use jni::objects::{JClass, JString};
use jni::sys::{jint, jstring};
use jni::JNIEnv;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use ssh2proxy_core::{Proxy, ProxyConfig};

static PROXY: OnceLock<tokio::sync::Mutex<Option<Proxy>>> = OnceLock::new();
static RT: OnceLock<Runtime> = OnceLock::new();

fn proxy_mutex() -> &'static tokio::sync::Mutex<Option<Proxy>> {
    PROXY.get_or_init(|| tokio::sync::Mutex::new(None))
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
    let rt = match runtime() {
        Some(rt) => rt,
        None => return -2,
    };
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut proxy = Proxy::new(config, tx);
    match rt.block_on(proxy.connect()) {
        Ok(()) => {
            let mut slot = proxy_mutex().blocking_lock();
            *slot = Some(proxy);
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
    let _raw: RawFd = fd;
    // 数据面 worker（tun::create_as_async(raw_fd) → Tun2Socks poll 循环 → DNS 拦截）
    // 在独立的「P1 数据面」计划中实现；此处仅接收 fd 并记录日志。
    log::info!("tun fd received: {fd}");
}

#[no_mangle]
pub extern "system" fn Java_com_wuko233_ssh2proxy_NativeBridge_disconnect(
    _env: JNIEnv,
    _class: JClass,
) {
    let rt = match runtime() {
        Some(rt) => rt,
        None => return,
    };
    let mut slot = proxy_mutex().blocking_lock();
    if let Some(mut proxy) = slot.take() {
        rt.block_on(proxy.disconnect());
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
