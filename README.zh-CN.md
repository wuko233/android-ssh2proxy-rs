# SSH2Proxy

[English](README.md) | [简体中文](README.zh-CN.md)

[![Build](https://github.com/wuko233/android-ssh2proxy-rs/actions/workflows/build.yml/badge.svg)](https://github.com/wuko233/android-ssh2proxy-rs/actions/workflows/build.yml)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

一个基于 **Rust 内核 + Jetpack Compose UI** 的 Android **SSH 隧道 / 全局代理**应用。

它先与你自己的服务器建立加密 SSH 连接，再把设备流量导向该服务器：既可以接管全部流量
（通过 Android `VpnService` / TUN，全局代理），也可以**只暴露本地 SOCKS5/HTTP 代理**、不建立 VPN。

繁重的部分（IP 包处理、TCP 状态机、DNS、UDP 中继、SSH）都用 Rust 编写并编译成单个 `.so`；
Kotlin 只是一层很薄的 UI + VPN 外壳。

```
Android App 流量
  → VpnService (TUN, Kotlin)
  → Rust 数据面（IPv4 + TCP 状态机 + 重传/流控）
  → 本地 SOCKS5 客户端 ──► 本地 SOCKS5 服务 (127.0.0.1:1080, Rust)
  → russh direct-tcpip channel
  → 你的 SSH 服务器 ──► 公网
```

## 功能

- **SSH 隧道**：基于 [`russh`](https://crates.io/crates/russh)（密码认证），带 keepalive、
  空闲超时与自动重连（指数退避，最长 30 秒）。
- **两种运行模式**
  - **全局代理（VPN）**：通过 `VpnService` 接管全部 TCP/UDP 流量。
  - **仅本地代理**：不建立 VPN，只在本机开放 SOCKS5/HTTP 代理。
- **本地代理服务**：SOCKS5（默认 `1080`）与 HTTP（默认 `8888`），端口可配置。
- **局域网共享**：将本地代理监听在 `0.0.0.0`，供局域网内其他设备使用。
- **DNS 走隧道**：拦截 DNS 查询并在远端解析，避免 DNS 泄漏。
- **UDP 转发**（QUIC、游戏语音）：以长度前缀帧的形式，经 SSH exec 通道上的内嵌 Python 中继转发。
- **多套配置**，支持**备注**（有备注时主页只显示备注名）与每套配置独立的 DNS 服务器。
- **连通性测试**：对可配置域名（默认 `www.baidu.com`）测试隧道 TCP、DNS 解析与 443 端口连通性。
- **延迟测试**：对可配置的 `host:port` 目标（默认 `223.5.5.5:53`）测试 SSH 往返与代理出口 TCP 延迟。
- **实时流量统计**（上下行字节数与速率），显示在主页与前台服务通知中。
- **日志查看**，使用简洁的 `[HH:mm]` 时间戳。
- **配置备份 / 恢复**：通过系统文件选择器，把所有配置与设置导出/导入为 JSON。
- **中英文界面**，可在应用内切换语言。

## 环境要求

**Android 设备**

- Android 7.0（API 24）及以上，`arm64-v8a`。
- 一台你有权使用、且可连通的 SSH 服务器（密码登录）。

**构建环境**

- [Rust](https://rustup.rs/)（stable）以及 `cargo-ndk`（`cargo install cargo-ndk --locked`）
- Android SDK + NDK（本项目在 **NDK 29.0.14206865**、compile SDK 37 下验证）
- JDK 17 及以上（在 JDK 21 下验证）

## 构建

Android 构建需要先把 Rust 内核交叉编译到 `app/src/main/jniLibs`，Gradle 才能打包 APK。

```bash
# 1. 在主机上测试并检查 Rust 内核
cargo test -p ssh2proxy-core
cargo clippy --workspace --all-targets -- -D warnings

# 2. 交叉编译 Rust 内核到 Android
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/29.0.14206865"
cargo ndk -t arm64-v8a -o app/src/main/jniLibs build -p ssh2proxy-android --release

# 3. 构建 debug APK
cd app && ./gradlew assembleDebug
```

APK 输出位置：

```
app/build/outputs/apk/debug/ssh2proxy-debug.apk
```

安装到已连接设备：

```bash
adb install -r app/build/outputs/apk/debug/ssh2proxy-debug.apk
```

### 预编译产物

每次 push 和 pull request 都会由
[GitHub Actions 工作流](.github/workflows/build.yml) 自动构建。在 **Actions** 页面打开最近一次
运行，下载 `ssh2proxy-debug-apk` 产物即可，无需本地工具链。

## 使用

1. 在主页点击 **＋**，填写 SSH 服务器信息：主机、端口、用户名、密码和 DNS 服务器
   （建议 `223.5.5.5` 或 `8.8.8.8`；该 DNS 主机必须能被 **SSH 服务器** 访问到）。
2. 选中该配置并点击 **连接**，按提示授权 VPN。
3. 连接成功后流量经 SSH 服务器转发，主页会显示实时上下行统计。
4. 可用 **连通性测试** 与 **测试延迟** 验证隧道。

### 仅本地代理模式

在 **设置 → 网络 → 仅本地代理** 打开开关即可跳过 VPN。连接后主页会显示 SOCKS5/HTTP 地址，
可在任意 App 中填写。开启 **局域网共享** 可让局域网内其他设备使用。

### UDP 支持

UDP 转发会在 SSH 服务器上运行一段 Python 3 脚本，由客户端通过 SSH exec 通道自动启动
（脚本内嵌在客户端中，服务器端无需安装任何文件）。**服务器上必须存在 `python3`。**
如果没有，UDP 会被禁用，而 TCP/DNS 仍可正常使用。

## 项目结构

| 路径 | 说明 |
|---|---|
| `core/` | 纯 Rust 代理内核：SSH、SOCKS5、HTTP 代理、数据面、TCP 状态机、DNS、UDP、探测。不依赖 Android。 |
| `android/src/lib.rs` | JNI 入口：连接/断开、TUN fd、流量统计、事件、延迟/连通性测试。 |
| `app/src/main/java/com/wuko233/ssh2proxy/` | Kotlin UI（`MainActivity.kt`）、VPN 服务、存储、JNI 桥、语言辅助。 |
| `app/src/main/res/values*/strings.xml` | 英文（默认）与中文界面字符串。 |
| `server/udprelay.py` | 服务器端 UDP-over-SSH 中继脚本（以 base64 内嵌到客户端）。 |
| `docs/` | 设计文档与实施计划。 |
| `.github/workflows/build.yml` | CI：Rust 测试/clippy + Android APK 构建。 |

## 说明与限制

- 仅支持 IPv4，不向 VPN 公告 IPv6。
- 仅支持密码认证（未实现私钥认证与 TOFU 主机密钥校验）。
- 未实现分应用代理（split tunneling）；除本应用外的所有应用都会走隧道。
- 仓库提交了预编译的 `app/src/main/jniLibs/arm64-v8a/libssh2proxy.so`，便于 Gradle 直接打包。
  修改 Rust 代码后请按上面的步骤重新生成。

## 许可证

基于 [Apache License 2.0](LICENSE) 许可。

Copyright 2026 wuko233。
