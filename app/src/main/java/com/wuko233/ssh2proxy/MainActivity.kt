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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.List
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
import java.util.UUID

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
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

        val selected = profiles.firstOrNull { it.id == selectedId }

        fun doConnect(p: Profile) {
            val auth = JSONObject().put("type", "password").put("password", p.password)
            val cfg = JSONObject().put("host", p.host).put("port", p.port)
                .put("username", p.username).put("auth", auth)
                .put("dns_server", p.dnsServer).toString()
            if (NativeBridge.connect(cfg) == 0) {
                ctx.startService(Intent(ctx, SshVpnService::class.java))
                connected = true
                status = "已连接"
            } else {
                status = "连接失败（请检查主机/端口/账号）"
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
            status = "已断开"
            testResult = null
        }

        fun runTest() {
            scope.launch {
                testing = true
                testResult = null
                val json = withContext(Dispatchers.IO) {
                    NativeBridge.runConnectivityTest("www.baidu.com")
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
                        icon = { Icon(Icons.Filled.List, contentDescription = "日志") },
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
                            if (status.isNotEmpty()) {
                                Surface(
                                    modifier = Modifier.fillMaxWidth(),
                                    shape = RoundedCornerShape(8.dp),
                                    color = if (connected) MaterialTheme.colorScheme.primaryContainer
                                    else MaterialTheme.colorScheme.surfaceVariant
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
                                enabled = connected || selected != null,
                                modifier = Modifier.fillMaxWidth().height(48.dp)
                            ) {
                                Text(if (connected) "断开连接" else "连接", fontWeight = FontWeight.Bold)
                            }

                            if (connected) {
                                Spacer(Modifier.height(8.dp))
                                Button(
                                    onClick = { runTest() },
                                    enabled = !testing,
                                    modifier = Modifier.fillMaxWidth().height(44.dp)
                                ) {
                                    Text(if (testing) "测试中…" else "连通性测试", fontWeight = FontWeight.Medium)
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
                        }
                    }
                    1 -> LogTab()
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

@Composable
fun LogTab() {
    var lines by remember { mutableStateOf(listOf<String>()) }

    LaunchedEffect(Unit) {
        while (true) {
            try {
                val json = NativeBridge.pollEvents()
                if (!json.isNullOrEmpty() && json != "[]") {
                    val arr = JSONArray(json)
                    val new = (0 until arr.length()).map { arr.getString(it) }
                    if (new.isNotEmpty()) {
                        lines = (lines + new).takeLast(1000)
                    }
                }
            } catch (_: Exception) {
            }
            delay(400)
        }
    }

    val listState = rememberLazyListState()
    LaunchedEffect(lines.size) {
        if (lines.isNotEmpty()) listState.scrollToItem(lines.size - 1)
    }

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.End
        ) {
            TextButton(onClick = { lines = emptyList() }) { Text("清空") }
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
    var cleared by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().padding(16.dp)) {
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