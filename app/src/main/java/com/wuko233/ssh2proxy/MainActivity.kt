package com.wuko233.ssh2proxy

import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.net.Inet4Address
import java.net.NetworkInterface
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.UUID

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        NativeBridge.setLogLevel(SettingsStore.logDebug(applicationContext))
        setContent { App() }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun App() {
    MaterialTheme(colorScheme = lightColorScheme()) {
        val ctx = LocalContext.current
        val scope = rememberCoroutineScope()
        var tab by remember { mutableStateOf(0) }
        var profiles by remember { mutableStateOf(ProfileStore.load(ctx)) }
        var selectedId by remember { mutableStateOf(profiles.firstOrNull()?.id) }
        var connected by remember { mutableStateOf(false) }
        var status by remember { mutableStateOf("") }
        var editing by remember { mutableStateOf<Profile?>(null) }
        var showAdd by remember { mutableStateOf(false) }
        var testResult by remember { mutableStateOf<String?>(null) }
        var testing by remember { mutableStateOf(false) }
        var latencyResult by remember { mutableStateOf<String?>(null) }
        var latencyTesting by remember { mutableStateOf(false) }
        var trafficUp by remember { mutableStateOf(0L) }
        var trafficDown by remember { mutableStateOf(0L) }
        var logLines by remember { mutableStateOf(listOf<String>()) }
        var localMode by remember { mutableStateOf(false) }
        var connecting by remember { mutableStateOf(false) }
        val localIp = remember { getLocalIp() }

        val selected = profiles.firstOrNull { it.id == selectedId }
        val sdf = remember { SimpleDateFormat("HH:mm", Locale.US) }

        // 持续拉取日志（提升到全局，避免切页丢日志）
        LaunchedEffect(Unit) {
            while (true) {
                try {
                    val json = NativeBridge.pollEvents()
                    if (!json.isNullOrEmpty() && json != "[]") {
                        val arr = JSONArray(json)
                        val new = (0 until arr.length()).map { i ->
                            val pair = arr.getJSONArray(i)
                            "[${sdf.format(Date(pair.getLong(0)))}] ${pair.getString(1)}"
                        }
                        if (new.isNotEmpty()) {
                            logLines = (logLines + new).takeLast(1000)
                        }
                    }
                } catch (_: Exception) {
                }
                delay(1000)
            }
        }

        LaunchedEffect(connected) {
            while (connected) {
                try {
                    val o = JSONObject(NativeBridge.getStats())
                    trafficUp = o.optLong("up")
                    trafficDown = o.optLong("down")
                } catch (_: Exception) {
                }
                delay(1000)
            }
            if (!connected) {
                trafficUp = 0L
                trafficDown = 0L
            }
        }

        fun runLatencyProbe() {
            if (latencyTesting) return
            scope.launch {
                latencyTesting = true
                latencyResult = null
                val json = withContext(Dispatchers.IO) {
                    NativeBridge.runLatencyTest(SettingsStore.testTarget(ctx))
                }
                latencyResult = formatLatencyResult(json)
                latencyTesting = false
            }
        }

        fun doConnect(p: Profile) {
            if (connecting) return
            val auth = JSONObject().put("type", "password").put("password", p.password)
            val bind = if (SettingsStore.lanShare(ctx)) "0.0.0.0" else "127.0.0.1"
            val cfg = JSONObject().put("host", p.host).put("port", p.port)
                .put("username", p.username).put("auth", auth)
                .put("dns_server", p.dnsServer)
                .put("udp_enabled", SettingsStore.udpEnabled(ctx))
                .put("bind_addr", bind)
                .put("socks_port", SettingsStore.socksPort(ctx))
                .put("http_port", SettingsStore.httpPort(ctx)).toString()
            scope.launch {
                connecting = true
                status = "连接中…"
                val code = withContext(Dispatchers.IO) {
                    NativeBridge.connect(cfg)
                }
                if (code == 0) {
                    if (SettingsStore.localProxyMode(ctx)) {
                        localMode = true
                        connected = true
                        status = ""
                    } else {
                        localMode = false
                        ctx.startService(Intent(ctx, SshVpnService::class.java))
                        connected = true
                        status = ""
                    }
                    runLatencyProbe()
                } else {
                    status = "连接失败（请检查主机/端口/账号）"
                }
                connecting = false
            }
        }

        val launcher = rememberLauncherForActivityResult(
            ActivityResultContracts.StartActivityForResult()
        ) { result ->
            val p = selected
            if (result.resultCode == android.app.Activity.RESULT_OK && p != null) {
                doConnect(p)
            }
        }

        fun connect() {
            val p = selected ?: return
            val intent = VpnService.prepare(ctx)
            if (intent != null) {
                launcher.launch(intent)
            } else {
                doConnect(p)
            }
        }

        fun disconnect() {
            NativeBridge.disconnect()
            NativeBridge.closeTun()
            ctx.stopService(Intent(ctx, SshVpnService::class.java))
            connected = false
            localMode = false
            status = "已断开"
            testResult = null
        }

        fun runTest() {
            scope.launch {
                testing = true
                testResult = null
                val json = withContext(Dispatchers.IO) {
                    NativeBridge.runConnectivityTest(SettingsStore.testDomain(ctx))
                }
                testResult = formatProbeResult(json)
                testing = false
            }
        }

        Scaffold(
            topBar = {
                TopAppBar(
                    title = {
                        Text(
                            when (tab) {
                                0 -> "SSH2Proxy"
                                1 -> "运行日志"
                                else -> "设置"
                            },
                            fontWeight = FontWeight.Bold
                        )
                    }
                )
            },
            bottomBar = {
                NavigationBar {
                    NavigationBarItem(
                        selected = tab == 0,
                        onClick = { tab = 0 },
                        icon = { Icon(Icons.Filled.Home, contentDescription = "主页") },
                        label = { Text("主页") }
                    )
                    NavigationBarItem(
                        selected = tab == 1,
                        onClick = { tab = 1 },
                        icon = { Icon(Icons.AutoMirrored.Filled.List, contentDescription = "日志") },
                        label = { Text("日志") }
                    )
                    NavigationBarItem(
                        selected = tab == 2,
                        onClick = { tab = 2 },
                        icon = { Icon(Icons.Filled.Settings, contentDescription = "设置") },
                        label = { Text("设置") }
                    )
                }
            },
            floatingActionButton = {
                if (tab == 0) {
                    FloatingActionButton(onClick = { showAdd = true }) { Text("＋", style = MaterialTheme.typography.titleLarge) }
                }
            }
        ) { padding ->
            Box(Modifier.padding(padding).fillMaxSize()) {
                when (tab) {
                    0 -> {
                        Column(Modifier.fillMaxSize().padding(16.dp)) {
                            Text("本机 IP：$localIp", style = MaterialTheme.typography.bodySmall)
                            Spacer(Modifier.height(8.dp))
                            if (connected) {
                                if (localMode) {
                                    val lan = SettingsStore.lanShare(ctx)
                                    val socksHost = if (lan) localIp else "127.0.0.1"
                                    val httpHost = if (lan) localIp else "127.0.0.1"
                                    val socksPort = SettingsStore.socksPort(ctx)
                                    val httpPort = SettingsStore.httpPort(ctx)
                                    Card(
                                        modifier = Modifier.fillMaxWidth(),
                                        shape = RoundedCornerShape(12.dp),
                                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer)
                                    ) {
                                        Column(Modifier.fillMaxWidth().padding(16.dp)) {
                                            Text("本地代理已启动", fontWeight = FontWeight.Bold, style = MaterialTheme.typography.titleMedium)
                                            Spacer(Modifier.height(8.dp))
                                            Text("SOCKS5: $socksHost:$socksPort", style = MaterialTheme.typography.bodyMedium)
                                            Text("HTTP:   $httpHost:$httpPort", style = MaterialTheme.typography.bodyMedium)
                                            Spacer(Modifier.height(8.dp))
                                            Text(if (lan) "局域网内其他设备可连接上面的地址" else "仅本机可用，局域网共享请在设置开启", style = MaterialTheme.typography.bodySmall)
                                        }
                                    }
                                } else {
                                    Card(
                                        modifier = Modifier.fillMaxWidth(),
                                        shape = RoundedCornerShape(12.dp),
                                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer)
                                    ) {
                                        Column(Modifier.fillMaxWidth().padding(16.dp)) {
                                            Text("已连接", fontWeight = FontWeight.Bold, style = MaterialTheme.typography.titleMedium)
                                            Spacer(Modifier.height(12.dp))
                                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                                                Column(Modifier.weight(1f)) {
                                                    Text("上传", style = MaterialTheme.typography.bodySmall)
                                                    Text(formatBytes(trafficUp), fontWeight = FontWeight.Bold, style = MaterialTheme.typography.titleLarge)
                                                }
                                                VerticalDivider(Modifier.height(40.dp))
                                                Column(Modifier.weight(1f)) {
                                                    Text("下载", style = MaterialTheme.typography.bodySmall)
                                                    Text(formatBytes(trafficDown), fontWeight = FontWeight.Bold, style = MaterialTheme.typography.titleLarge)
                                                }
                                            }
                                        }
                                    }
                                }
                                Spacer(Modifier.height(12.dp))
                            } else if (status.isNotEmpty()) {
                                Surface(
                                    modifier = Modifier.fillMaxWidth(),
                                    shape = RoundedCornerShape(8.dp),
                                    color = MaterialTheme.colorScheme.surfaceVariant
                                ) {
                                    Text(
                                        "状态：$status",
                                        modifier = Modifier.padding(12.dp),
                                        fontWeight = FontWeight.Medium
                                    )
                                }
                                Spacer(Modifier.height(12.dp))
                            }

                            if (profiles.isEmpty()) {
                                Box(Modifier.weight(0.7f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                                    Text("还没有 SSH 配置\n点右下角 ＋ 添加", style = MaterialTheme.typography.bodyLarge)
                                }
                            } else {
                                LazyColumn(Modifier.weight(0.7f)) {
                                    items(profiles, key = { it.id }) { p ->
                                        ProfileCard(
                                            profile = p,
                                            selected = p.id == selectedId,
                                            onClick = { selectedId = p.id; status = "" },
                                            onEdit = { editing = p },
                                            onDelete = {
                                                profiles = profiles.filter { it.id != p.id }
                                                ProfileStore.save(ctx, profiles)
                                                if (selectedId == p.id) selectedId = profiles.firstOrNull()?.id
                                            }
                                        )
                                    }
                                }
                            }

                            Spacer(Modifier.height(8.dp))
                            Button(
                                onClick = { if (connected) disconnect() else connect() },
                                enabled = (connected || selected != null) && !connecting,
                                modifier = Modifier.fillMaxWidth().height(48.dp)
                            ) {
                                Text(
                                    when {
                                        connecting -> "连接中…"
                                        connected -> "断开连接"
                                        else -> "连接"
                                    },
                                    fontWeight = FontWeight.Bold
                                )
                            }

                            if (connected) {
                                Spacer(Modifier.height(8.dp))
                                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    Button(
                                        onClick = { runTest() },
                                        enabled = !testing,
                                        modifier = Modifier.weight(1f).height(44.dp)
                                    ) {
                                        Text(if (testing) "测试中…" else "连通性测试", fontWeight = FontWeight.Medium)
                                    }
                                    OutlinedButton(
                                        onClick = { runLatencyProbe() },
                                        enabled = !latencyTesting,
                                        modifier = Modifier.weight(1f).height(44.dp)
                                    ) {
                                        Text(if (latencyTesting) "延迟测试中…" else "测试延迟", fontWeight = FontWeight.Medium)
                                    }
                                }
                            }

                            testResult?.let {
                                Spacer(Modifier.height(8.dp))
                                SelectionContainer {
                                    Surface(
                                        modifier = Modifier.fillMaxWidth(),
                                        shape = RoundedCornerShape(8.dp),
                                        color = MaterialTheme.colorScheme.secondaryContainer
                                    ) {
                                        Text(it, modifier = Modifier.padding(12.dp), style = MaterialTheme.typography.bodySmall)
                                    }
                                }
                            }

                            latencyResult?.let {
                                Spacer(Modifier.height(8.dp))
                                SelectionContainer {
                                    Surface(
                                        modifier = Modifier.fillMaxWidth(),
                                        shape = RoundedCornerShape(12.dp),
                                        color = MaterialTheme.colorScheme.tertiaryContainer
                                    ) {
                                        Text(it, modifier = Modifier.padding(14.dp), style = MaterialTheme.typography.bodyMedium)
                                    }
                                }
                            }
                        }
                    }
                    1 -> LogTab(logLines) { logLines = emptyList() }
                    else -> SettingsTab()
                }
            }
        }

        if (showAdd) {
            ProfileFormDialog(
                existing = null,
                onSave = { p ->
                    profiles = profiles + p
                    ProfileStore.save(ctx, profiles)
                    selectedId = p.id
                    showAdd = false
                },
                onDismiss = { showAdd = false }
            )
        }
        editing?.let { p ->
            ProfileFormDialog(
                existing = p,
                onSave = { updated ->
                    profiles = profiles.map { if (it.id == updated.id) updated else it }
                    ProfileStore.save(ctx, profiles)
                    selectedId = updated.id
                    editing = null
                },
                onDismiss = { editing = null }
            )
        }
    }
}

fun formatProbeResult(json: String?): String {
    if (json.isNullOrEmpty()) return "测试失败（无响应）"
    return try {
        val o = JSONObject(json)
        val socksTcp = o.optBoolean("socks_tcp_ok")
        val dnsOk = o.optBoolean("dns_ok")
        val ips = o.optJSONArray("resolved_ips")
        val ipsStr = if (ips != null && ips.length() > 0) {
            (0 until ips.length()).joinToString(", ") { ips.getString(it) }
        } else "无"
        val tcpOk = o.optBoolean("tcp_connect_ok")
        val err = o.optString("error")
        buildString {
            append("域名: ").append(o.optString("domain")).append('\n')
            append("隧道TCP连通: ").append(if (socksTcp) "✓" else "✗").append('\n')
            append("DNS 解析: ").append(if (dnsOk) "✓ 成功" else "✗ 失败").append('\n')
            append("解析 IP: ").append(ipsStr).append('\n')
            append("TCP 连通(443): ").append(if (tcpOk) "✓ 成功" else "✗ 失败")
            if (err.isNotEmpty()) append('\n').append("错误: ").append(err)
        }
    } catch (_: Exception) {
        "测试失败: $json"
    }
}

fun formatLatencyResult(json: String?): String {
    if (json.isNullOrEmpty()) return "延迟测试失败（无响应）"
    return try {
        val o = JSONObject(json)
        val sshText = if (o.optBoolean("ssh_ok")) {
            "${o.optLong("ssh_latency_ms")} ms"
        } else {
            "失败：${o.optString("ssh_error", "未知错误")}"
        }
        val proxyText = if (o.optBoolean("proxy_ok")) {
            "${o.optLong("proxy_latency_ms")} ms"
        } else {
            "失败：${o.optString("proxy_error", "未知错误")}"
        }
        "延迟测试\nSSH 服务器：$sshText\n代理出口：$proxyText"
    } catch (_: Exception) {
        "延迟测试失败：$json"
    }
}

fun formatBytes(b: Long): String {
    val kb = 1024.0
    val mb = kb * 1024
    val gb = mb * 1024
    return when {
        b >= gb -> String.format(Locale.US, "%.2f GB", b / gb)
        b >= mb -> String.format(Locale.US, "%.2f MB", b / mb)
        b >= kb -> String.format(Locale.US, "%.1f KB", b / kb)
        else -> "$b B"
    }
}

fun getLocalIp(): String {
    try {
        val interfaces = NetworkInterface.getNetworkInterfaces()
        while (interfaces.hasMoreElements()) {
            val nif = interfaces.nextElement()
            val addrs = nif.inetAddresses
            while (addrs.hasMoreElements()) {
                val addr = addrs.nextElement()
                if (!addr.isLoopbackAddress && addr is Inet4Address) {
                    val host = addr.hostAddress
                    if (host != null && !host.startsWith("169.254.")) {
                        return host
                    }
                }
            }
        }
    } catch (_: Exception) {
    }
    return "未知"
}

@Composable
fun LogTab(lines: List<String>, onClear: () -> Unit) {
    val listState = rememberLazyListState()
    LaunchedEffect(lines.size) {
        if (lines.isNotEmpty()) listState.scrollToItem(lines.size - 1)
    }

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.End
        ) {
            TextButton(onClick = onClear) { Text("清空") }
        }
        if (lines.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("暂无日志", style = MaterialTheme.typography.bodyLarge)
            }
        } else {
            SelectionContainer {
                LazyColumn(Modifier.fillMaxSize(), state = listState) {
                    items(lines) { line ->
                        Text(
                            line,
                            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 2.dp),
                            style = MaterialTheme.typography.bodySmall
                        )
                    }
                }
            }
        }
    }
}

@Composable
fun SettingsTab() {
    val ctx = LocalContext.current
    var udp by remember { mutableStateOf(SettingsStore.udpEnabled(ctx)) }
    var logDebug by remember { mutableStateOf(SettingsStore.logDebug(ctx)) }
    var localProxy by remember { mutableStateOf(SettingsStore.localProxyMode(ctx)) }
    var lanShare by remember { mutableStateOf(SettingsStore.lanShare(ctx)) }
    var socksPortStr by remember { mutableStateOf(SettingsStore.socksPort(ctx).toString()) }
    var httpPortStr by remember { mutableStateOf(SettingsStore.httpPort(ctx).toString()) }
    var testDomain by remember { mutableStateOf(SettingsStore.testDomain(ctx)) }
    var testTarget by remember { mutableStateOf(SettingsStore.testTarget(ctx)) }
    var cleared by remember { mutableStateOf(false) }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
        Text("网络", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("仅本地代理", style = MaterialTheme.typography.bodyLarge)
                Text("连接后只开放本地 SOCKS5/HTTP 代理，不建立 VPN", style = MaterialTheme.typography.bodySmall)
            }
            Switch(checked = localProxy, onCheckedChange = {
                localProxy = it
                SettingsStore.setLocalProxyMode(ctx, it)
            })
        }
        Spacer(Modifier.height(12.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("局域网共享", style = MaterialTheme.typography.bodyLarge)
                Text("监听 0.0.0.0，让局域网内其他设备也能用", style = MaterialTheme.typography.bodySmall)
            }
            Switch(checked = lanShare, onCheckedChange = {
                lanShare = it
                SettingsStore.setLanShare(ctx, it)
            })
        }
        Spacer(Modifier.height(12.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("UDP 转发", style = MaterialTheme.typography.bodyLarge)
                Text("通过 SSH 隧道转发 UDP 流量（QUIC/游戏语音）", style = MaterialTheme.typography.bodySmall)
            }
            Switch(checked = udp, onCheckedChange = {
                udp = it
                SettingsStore.setUdpEnabled(ctx, it)
            })
        }
        Spacer(Modifier.height(16.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(
                value = socksPortStr,
                onValueChange = {
                    socksPortStr = it
                    it.toIntOrNull()?.let { p -> SettingsStore.setSocksPort(ctx, p) }
                },
                label = { Text("SOCKS5 端口") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                modifier = Modifier.weight(1f)
            )
            OutlinedTextField(
                value = httpPortStr,
                onValueChange = {
                    httpPortStr = it
                    it.toIntOrNull()?.let { p -> SettingsStore.setHttpPort(ctx, p) }
                },
                label = { Text("HTTP 端口") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                modifier = Modifier.weight(1f)
            )
        }
        Spacer(Modifier.height(16.dp))
        HorizontalDivider()
        Spacer(Modifier.height(16.dp))

        Text("测试", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = testDomain,
            onValueChange = {
                testDomain = it
                SettingsStore.setTestDomain(ctx, it)
            },
            label = { Text("连通性测试域名") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth()
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = testTarget,
            onValueChange = {
                testTarget = it
                SettingsStore.setTestTarget(ctx, it)
            },
            label = { Text("出口延迟目标（host:port）") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth()
        )
        Spacer(Modifier.height(16.dp))
        HorizontalDivider()
        Spacer(Modifier.height(16.dp))

        Text("日志", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("详细日志", style = MaterialTheme.typography.bodyLarge)
                Text("输出 Debug 级别日志（排查问题时开启）", style = MaterialTheme.typography.bodySmall)
            }
            Switch(checked = logDebug, onCheckedChange = {
                logDebug = it
                SettingsStore.setLogDebug(ctx, it)
                NativeBridge.setLogLevel(it)
            })
        }
        Spacer(Modifier.height(16.dp))
        HorizontalDivider()
        Spacer(Modifier.height(16.dp))

        Text("关于", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Text("SSH2Proxy v0.1.0")
        Text("基于 Rust 的 SSH 全局代理")
        Spacer(Modifier.height(24.dp))
        OutlinedButton(onClick = {
            ProfileStore.save(ctx, emptyList())
            cleared = true
        }) {
            Text("清除所有配置")
        }
        if (cleared) {
            Spacer(Modifier.height(8.dp))
            Text("已清除，请回到主页重新添加")
        }
    }
}

@Composable
fun ProfileCard(profile: Profile, selected: Boolean, onClick: () -> Unit, onEdit: () -> Unit, onDelete: () -> Unit) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp)
            .clickable { onClick() },
        colors = CardDefaults.cardColors(
            containerColor = if (selected) MaterialTheme.colorScheme.primaryContainer
            else MaterialTheme.colorScheme.surfaceVariant
        )
    ) {
        Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(
                    "${profile.username}@${profile.host}:${profile.port}",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold
                )
                Text("DNS: ${profile.dnsServer}", style = MaterialTheme.typography.bodySmall)
            }
            TextButton(onClick = onEdit) { Text("编辑") }
            TextButton(onClick = onDelete) { Text("删除", color = MaterialTheme.colorScheme.error) }
        }
    }
}

@Composable
fun ProfileFormDialog(existing: Profile?, onSave: (Profile) -> Unit, onDismiss: () -> Unit) {
    var host by remember { mutableStateOf(existing?.host ?: "") }
    var port by remember { mutableStateOf(existing?.port?.toString() ?: "22") }
    var username by remember { mutableStateOf(existing?.username ?: "root") }
    var password by remember { mutableStateOf(existing?.password ?: "") }
    var dnsServer by remember { mutableStateOf(existing?.dnsServer ?: "8.8.8.8") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(if (existing == null) "添加 SSH 配置" else "编辑 SSH 配置") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(value = host, onValueChange = { host = it }, label = { Text("主机地址") }, singleLine = true)
                OutlinedTextField(
                    value = port, onValueChange = { port = it }, label = { Text("端口") },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number)
                )
                OutlinedTextField(value = username, onValueChange = { username = it }, label = { Text("用户名") }, singleLine = true)
                OutlinedTextField(
                    value = password, onValueChange = { password = it }, label = { Text("密码") },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation()
                )
                OutlinedTextField(
                    value = dnsServer, onValueChange = { dnsServer = it }, label = { Text("DNS 服务器（国内建议 223.5.5.5）") },
                    singleLine = true
                )
            }
        },
        confirmButton = {
            TextButton(onClick = {
                val p = Profile(
                    id = existing?.id ?: UUID.randomUUID().toString(),
                    host = host.trim(),
                    port = port.toIntOrNull() ?: 22,
                    username = username.trim(),
                    password = password,
                    dnsServer = dnsServer.trim().ifEmpty { "8.8.8.8" },
                )
                onSave(p)
            }) { Text("保存") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("取消") } }
    )
}
