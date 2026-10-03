# SSH2Proxy

[English](README.md) | [简体中文](README.zh-CN.md)

[![Build](https://github.com/wuko233/android-ssh2proxy-rs/actions/workflows/build.yml/badge.svg)](https://github.com/wuko233/android-ssh2proxy-rs/actions/workflows/build.yml)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

An Android **SSH tunnel / global proxy** app with a Rust core and a Jetpack Compose UI.

It establishes an encrypted SSH connection to a server you control, then routes the
device's traffic through it — either by taking over all traffic with Android's
`VpnService` (TUN), or by exposing a **local SOCKS5/HTTP proxy** without a VPN.

The heavy lifting (IP packet handling, TCP state machine, DNS, UDP relay, SSH)
is written in Rust and compiled into a single `.so`; Kotlin is a thin UI + VPN shell.

```
Android App traffic
  → VpnService (TUN, Kotlin)
  → Rust data plane (IPv4 + TCP state machine + retransmit/window control)
  → local SOCKS5 client ──► local SOCKS5 server (127.0.0.1:1080, Rust)
  → russh direct-tcpip channel
  → your SSH server ──► the internet
```

## Features

- **SSH tunnel** using [`russh`](https://crates.io/crates/russh) (password auth), with
  keepalive, idle timeout and automatic reconnect (exponential backoff, capped at 30 s).
- **Two operating modes**
  - **Global proxy (VPN)**: takes over all TCP/UDP traffic via `VpnService`.
  - **Local proxy only**: exposes SOCKS5 and HTTP proxies on the device without a VPN.
- **Local proxy servers**: SOCKS5 (`1080`) and HTTP (`8888`) with configurable ports.
- **LAN sharing**: bind the local proxies to `0.0.0.0` so other devices can use them.
- **DNS over the tunnel**: DNS queries are intercepted and resolved remotely (no leaks).
- **UDP forwarding** (QUIC, game voice): tunnelled as frames over an SSH exec channel
  running an embedded Python relay on the server.
- **Multiple profiles** with an optional **note/remark** (the note replaces the address on
  the home screen), plus per-profile DNS server.
- **Connectivity test**: tunnel TCP, DNS resolution and port-443 reachability for a
  configurable domain (default `www.baidu.com`).
- **Latency test**: SSH round-trip and proxy egress TCP latency against a configurable
  `host:port` target (default `223.5.5.5:53`).
- **Live traffic statistics** (up/down bytes and rate) on the home screen and in the
  foreground-service notification.
- **Log viewer** with concise `[HH:mm]` timestamps.
- **Config backup / restore**: export and import all profiles and settings as JSON through
  the system file picker.
- **English / Chinese UI** with an in-app language switch.

## Requirements

**Android device**

- Android 7.0 (API 24) or newer, `arm64-v8a`.
- A reachable SSH server (password login) you are allowed to use.

**Build machine**

- [Rust](https://rustup.rs/) (stable) and `cargo-ndk`
  (`cargo install cargo-ndk --locked`)
- Android SDK + NDK (project is tested with **NDK 29.0.14206865**, compile SDK 37)
- JDK 17 or newer (tested with JDK 21)

## Building

The Android build needs the Rust core cross-compiled into `app/src/main/jniLibs`
**before** Gradle packages the APK.

```bash
# 1. Test and lint the Rust core on the host
cargo test -p ssh2proxy-core
cargo clippy --workspace --all-targets -- -D warnings

# 2. Cross-compile the Rust core for Android
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/29.0.14206865"
cargo ndk -t arm64-v8a -o app/src/main/jniLibs build -p ssh2proxy-android --release

# 3. Build the debug APK
cd app && ./gradlew assembleDebug
```

The APK is written to:

```
app/build/outputs/apk/debug/ssh2proxy-debug.apk
```

Install it on a connected device:

```bash
adb install -r app/build/outputs/apk/debug/ssh2proxy-debug.apk
```

### Prebuilt artifacts

Every push and pull request is built by the
[GitHub Actions workflow](.github/workflows/build.yml). Download the
`ssh2proxy-debug-apk` artifact from the latest run on the **Actions** tab — no local
toolchain required.

## Usage

1. Tap **＋** on the Home tab and fill in your SSH server: host, port, username,
   password and DNS server (use `223.5.5.5` or `8.8.8.8`; the DNS host must be
   reachable **from the SSH server**).
2. Select the profile and tap **Connect**. Grant the VPN permission when prompted.
3. When connected, traffic is routed through the SSH server, and the home screen shows
   live upload/download statistics.
4. Use **Connectivity test** and **Latency test** to verify the tunnel.

### Local proxy mode

Enable **Settings → Network → Local proxy only** to skip the VPN. After connecting, the
app prints the SOCKS5/HTTP addresses on the home screen so you can configure them in
any app. Enable **LAN sharing** to expose them to your local network.

### UDP support

UDP forwarding runs a small Python 3 script on the SSH server, started automatically
over an SSH exec channel (the script is embedded in the client, so nothing needs to be
installed server-side). **`python3` must be available on the server.** If it is not, UDP
is disabled and TCP/DNS keep working.

## Project layout

| Path | Description |
|---|---|
| `core/` | Pure-Rust proxy core: SSH, SOCKS5, HTTP proxy, data plane, TCP state machine, DNS, UDP, probes. No Android dependencies. |
| `android/src/lib.rs` | JNI entry points: connect/disconnect, TUN fd, stats, events, latency/connectivity tests. |
| `app/src/main/java/com/wuko233/ssh2proxy/` | Kotlin UI (`MainActivity.kt`), VPN service, stores, JNI bridge, locale helper. |
| `app/src/main/res/values*/strings.xml` | English (default) and Chinese UI strings. |
| `server/udprelay.py` | Server-side UDP-over-SSH-channel relay (base64-embedded in the client). |
| `.github/workflows/build.yml` | CI: Rust tests/clippy + Android APK build. |

## Notes and limitations

- IPv4 only; IPv6 is not advertised to the VPN.
- Password authentication only (private-key and TOFU host-key verification are not
  implemented).
- Per-app routing (split tunneling) is not implemented; all apps except this one go
  through the tunnel.
- The repository commits the prebuilt `app/src/main/jniLibs/arm64-v8a/libssh2proxy.so`
  so Gradle can package it directly. Rebuild it with the steps above after changing Rust
  code.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).

Copyright 2026 wuko233.
