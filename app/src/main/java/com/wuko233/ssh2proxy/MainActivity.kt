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
import androidx.compose.ui.res.stringResource
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
    override fun attachBaseContext(newBase: Context) {
        super.attachBaseContext(LocaleHelper.wrap(newBase))
    }

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
                latencyResult = formatLatencyResult(ctx, json)
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
                status = ctx.getString(R.string.home_connecting)
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
                    status = ctx.getString(R.string.home_connect_failed)
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
            status = ctx.getString(R.string.home_disconnected)
            testResult = null
        }

        fun runTest() {
            scope.launch {
                testing = true
                testResult = null
                val json = withContext(Dispatchers.IO) {
                    NativeBridge.runConnectivityTest(SettingsStore.testDomain(ctx))
                }
                testResult = formatProbeResult(ctx, json)
                testing = false
            }
        }

        Scaffold(
            topBar = {
                TopAppBar(
                    title = {
                        Text(
                            when (tab) {
                                0 -> stringResource(R.string.title_home)
                                1 -> stringResource(R.string.title_log)
                                else -> stringResource(R.string.title_settings)
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
                        icon = { Icon(Icons.Filled.Home, contentDescription = stringResource(R.string.tab_home)) },
                        label = { Text(stringResource(R.string.tab_home)) }
                    )
                    NavigationBarItem(
                        selected = tab == 1,
                        onClick = { tab = 1 },
                        icon = { Icon(Icons.AutoMirrored.Filled.List, contentDescription = stringResource(R.string.tab_log)) },
                        label = { Text(stringResource(R.string.tab_log)) }
                    )
                    NavigationBarItem(
                        selected = tab == 2,
                        onClick = { tab = 2 },
                        icon = { Icon(Icons.Filled.Settings, contentDescription = stringResource(R.string.tab_settings)) },
                        label = { Text(stringResource(R.string.tab_settings)) }
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
                            Text(stringResource(R.string.home_local_ip, localIp ?: stringResource(R.string.unknown)), style = MaterialTheme.typography.bodySmall)
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
                                            Text(stringResource(R.string.home_proxy_started), fontWeight = FontWeight.Bold, style = MaterialTheme.typography.titleMedium)
                                            Spacer(Modifier.height(8.dp))
                                            Text("SOCKS5: $socksHost:$socksPort", style = MaterialTheme.typography.bodyMedium)
                                            Text("HTTP:   $httpHost:$httpPort", style = MaterialTheme.typography.bodyMedium)
                                            Spacer(Modifier.height(8.dp))
                                            Text(if (lan) stringResource(R.string.home_lan_hint) else stringResource(R.string.home_local_only_hint), style = MaterialTheme.typography.bodySmall)
                                        }
                                    }
                                } else {
                                    Card(
                                        modifier = Modifier.fillMaxWidth(),
                                        shape = RoundedCornerShape(12.dp),
                                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer)
                                    ) {
                                        Column(Modifier.fillMaxWidth().padding(16.dp)) {
                                            Text(stringResource(R.string.home_connected), fontWeight = FontWeight.Bold, style = MaterialTheme.typography.titleMedium)
                                            Spacer(Modifier.height(12.dp))
                                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                                                Column(Modifier.weight(1f)) {
                                                    Text(stringResource(R.string.home_upload), style = MaterialTheme.typography.bodySmall)
                                                    Text(formatBytes(trafficUp), fontWeight = FontWeight.Bold, style = MaterialTheme.typography.titleLarge)
                                                }
                                                VerticalDivider(Modifier.height(40.dp))
                                                Column(Modifier.weight(1f)) {
                                                    Text(stringResource(R.string.home_download), style = MaterialTheme.typography.bodySmall)
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
                                        stringResource(R.string.home_status, status),
                                        modifier = Modifier.padding(12.dp),
                                        fontWeight = FontWeight.Medium
                                    )
                                }
                                Spacer(Modifier.height(12.dp))
                            }

                            if (profiles.isEmpty()) {
                                Box(Modifier.weight(0.7f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                                    Text(stringResource(R.string.home_no_profiles), style = MaterialTheme.typography.bodyLarge)
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
                                        connecting -> stringResource(R.string.home_connecting)
                                        connected -> stringResource(R.string.home_disconnect)
                                        else -> stringResource(R.string.home_connect)
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
                                        Text(if (testing) stringResource(R.string.home_testing) else stringResource(R.string.home_test_connectivity), fontWeight = FontWeight.Medium)
                                    }
                                    OutlinedButton(
                                        onClick = { runLatencyProbe() },
                                        enabled = !latencyTesting,
                                        modifier = Modifier.weight(1f).height(44.dp)
                                    ) {
                                        Text(if (latencyTesting) stringResource(R.string.home_testing_latency) else stringResource(R.string.home_test_latency), fontWeight = FontWeight.Medium)
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
                    else -> SettingsTab(onImported = {
                        profiles = ProfileStore.load(ctx)
                        selectedId = profiles.firstOrNull()?.id
                    })
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

fun formatProbeResult(ctx: Context, json: String?): String {
    if (json.isNullOrEmpty()) return ctx.getString(R.string.probe_failed_no_response)
    return try {
        val o = JSONObject(json)
        val socksTcp = o.optBoolean("socks_tcp_ok")
        val dnsOk = o.optBoolean("dns_ok")
        val ips = o.optJSONArray("resolved_ips")
        val ipsStr = if (ips != null && ips.length() > 0) {
            (0 until ips.length()).joinToString(", ") { ips.getString(it) }
        } else ctx.getString(R.string.probe_none)
        val tcpOk = o.optBoolean("tcp_connect_ok")
        val err = o.optString("error")
        val ok = ctx.getString(R.string.probe_ok)
        val fail = ctx.getString(R.string.probe_fail)
        buildString {
            append(ctx.getString(R.string.probe_domain, o.optString("domain"))).append('\n')
            append(ctx.getString(R.string.probe_tunnel_tcp, if (socksTcp) ok else fail)).append('\n')
            append(ctx.getString(R.string.probe_dns, if (dnsOk) ok else fail)).append('\n')
            append(ctx.getString(R.string.probe_resolved_ips, ipsStr)).append('\n')
            append(ctx.getString(R.string.probe_tcp_443, if (tcpOk) ok else fail))
            if (err.isNotEmpty()) append('\n').append(ctx.getString(R.string.probe_error, err))
        }
    } catch (_: Exception) {
        ctx.getString(R.string.probe_failed, json)
    }
}

fun formatLatencyResult(ctx: Context, json: String?): String {
    if (json.isNullOrEmpty()) return ctx.getString(R.string.latency_failed_no_response)
    return try {
        val o = JSONObject(json)
        val unknown = ctx.getString(R.string.latency_unknown_error)
        val sshText = if (o.optBoolean("ssh_ok")) {
            "${o.optLong("ssh_latency_ms")} ms"
        } else {
            ctx.getString(R.string.latency_failed_reason, o.optString("ssh_error", unknown))
        }
        val proxyText = if (o.optBoolean("proxy_ok")) {
            "${o.optLong("proxy_latency_ms")} ms"
        } else {
            ctx.getString(R.string.latency_failed_reason, o.optString("proxy_error", unknown))
        }
        ctx.getString(R.string.latency_result, sshText, proxyText)
    } catch (_: Exception) {
        ctx.getString(R.string.latency_failed, json)
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

fun getLocalIp(): String? {
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
    return null
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
            TextButton(onClick = onClear) { Text(stringResource(R.string.clear)) }
        }
        if (lines.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(stringResource(R.string.log_empty), style = MaterialTheme.typography.bodyLarge)
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
fun SettingsTab(onImported: () -> Unit) {
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
    var backupMsg by remember { mutableStateOf("") }
    var language by remember { mutableStateOf(SettingsStore.language(ctx)) }

    val exportLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        if (uri != null) {
            backupMsg = try {
                val json = BackupStore.exportJson(ctx)
                ctx.contentResolver.openOutputStream(uri)?.use { os ->
                    os.write(json.toByteArray(Charsets.UTF_8))
                }
                ctx.getString(R.string.settings_export_ok)
            } catch (e: Exception) {
                ctx.getString(R.string.settings_export_fail, e.message ?: "")
            }
        }
    }

    val importLauncher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            backupMsg = try {
                val json = ctx.contentResolver.openInputStream(uri)?.use { it.readBytes() }?.toString(Charsets.UTF_8)
                if (json != null && BackupStore.importJson(ctx, json)) {
                    onImported()
                    ctx.getString(R.string.settings_import_ok)
                } else {
                    ctx.getString(R.string.settings_import_invalid)
                }
            } catch (e: Exception) {
                ctx.getString(R.string.settings_import_fail, e.message ?: "")
            }
        }
    }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
        Text(stringResource(R.string.settings_network), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.settings_local_proxy), style = MaterialTheme.typography.bodyLarge)
                Text(stringResource(R.string.settings_local_proxy_desc), style = MaterialTheme.typography.bodySmall)
            }
            Switch(checked = localProxy, onCheckedChange = {
                localProxy = it
                SettingsStore.setLocalProxyMode(ctx, it)
            })
        }
        Spacer(Modifier.height(12.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.settings_lan_share), style = MaterialTheme.typography.bodyLarge)
                Text(stringResource(R.string.settings_lan_share_desc), style = MaterialTheme.typography.bodySmall)
            }
            Switch(checked = lanShare, onCheckedChange = {
                lanShare = it
                SettingsStore.setLanShare(ctx, it)
            })
        }
        Spacer(Modifier.height(12.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.settings_udp), style = MaterialTheme.typography.bodyLarge)
                Text(stringResource(R.string.settings_udp_desc), style = MaterialTheme.typography.bodySmall)
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
                label = { Text(stringResource(R.string.settings_socks_port)) },
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
                label = { Text(stringResource(R.string.settings_http_port)) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                modifier = Modifier.weight(1f)
            )
        }
        Spacer(Modifier.height(16.dp))
        HorizontalDivider()
        Spacer(Modifier.height(16.dp))

        Text(stringResource(R.string.settings_language), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilterChip(
                selected = language == LocaleHelper.SYSTEM,
                onClick = {
                    language = LocaleHelper.SYSTEM
                    SettingsStore.setLanguage(ctx, LocaleHelper.SYSTEM)
                    ctx.findActivity()?.recreate()
                },
                label = { Text(stringResource(R.string.settings_language_system)) }
            )
            FilterChip(
                selected = language == LocaleHelper.ZH,
                onClick = {
                    language = LocaleHelper.ZH
                    SettingsStore.setLanguage(ctx, LocaleHelper.ZH)
                    ctx.findActivity()?.recreate()
                },
                label = { Text(stringResource(R.string.settings_language_zh)) }
            )
            FilterChip(
                selected = language == LocaleHelper.EN,
                onClick = {
                    language = LocaleHelper.EN
                    SettingsStore.setLanguage(ctx, LocaleHelper.EN)
                    ctx.findActivity()?.recreate()
                },
                label = { Text(stringResource(R.string.settings_language_en)) }
            )
        }
        Spacer(Modifier.height(16.dp))
        HorizontalDivider()
        Spacer(Modifier.height(16.dp))

        Text(stringResource(R.string.settings_test), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = testDomain,
            onValueChange = {
                testDomain = it
                SettingsStore.setTestDomain(ctx, it)
            },
            label = { Text(stringResource(R.string.settings_test_domain)) },
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
            label = { Text(stringResource(R.string.settings_test_target)) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth()
        )
        Spacer(Modifier.height(16.dp))
        HorizontalDivider()
        Spacer(Modifier.height(16.dp))

        Text(stringResource(R.string.settings_log), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.settings_verbose_log), style = MaterialTheme.typography.bodyLarge)
                Text(stringResource(R.string.settings_verbose_log_desc), style = MaterialTheme.typography.bodySmall)
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

        Text(stringResource(R.string.settings_about), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Text("SSH2Proxy v0.1.0")
        Text(stringResource(R.string.settings_about_desc))
        Spacer(Modifier.height(24.dp))

        Text(stringResource(R.string.settings_backup), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedButton(
                onClick = { exportLauncher.launch("ssh2proxy-backup.json") },
                modifier = Modifier.weight(1f)
            ) { Text(stringResource(R.string.settings_export)) }
            OutlinedButton(
                onClick = { importLauncher.launch(arrayOf("application/json", "text/*", "*/*")) },
                modifier = Modifier.weight(1f)
            ) { Text(stringResource(R.string.settings_import)) }
        }
        if (backupMsg.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            Text(backupMsg, style = MaterialTheme.typography.bodySmall)
        }
        Spacer(Modifier.height(16.dp))

        OutlinedButton(onClick = {
            ProfileStore.save(ctx, emptyList())
            cleared = true
        }) {
            Text(stringResource(R.string.settings_clear_all))
        }
        if (cleared) {
            Spacer(Modifier.height(8.dp))
            Text(stringResource(R.string.settings_cleared))
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
                    if (profile.note.isNotBlank()) profile.note else "${profile.username}@${profile.host}:${profile.port}",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold
                )
                Text(stringResource(R.string.profile_dns_label, profile.dnsServer), style = MaterialTheme.typography.bodySmall)
            }
            TextButton(onClick = onEdit) { Text(stringResource(R.string.edit)) }
            TextButton(onClick = onDelete) { Text(stringResource(R.string.delete), color = MaterialTheme.colorScheme.error) }
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
    var note by remember { mutableStateOf(existing?.note ?: "") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(if (existing == null) stringResource(R.string.profile_add_title) else stringResource(R.string.profile_edit_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(value = host, onValueChange = { host = it }, label = { Text(stringResource(R.string.profile_host)) }, singleLine = true)
                OutlinedTextField(
                    value = port, onValueChange = { port = it }, label = { Text(stringResource(R.string.profile_port)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number)
                )
                OutlinedTextField(value = username, onValueChange = { username = it }, label = { Text(stringResource(R.string.profile_username)) }, singleLine = true)
                OutlinedTextField(
                    value = password, onValueChange = { password = it }, label = { Text(stringResource(R.string.profile_password)) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation()
                )
                OutlinedTextField(
                    value = dnsServer, onValueChange = { dnsServer = it }, label = { Text(stringResource(R.string.profile_dns)) },
                    singleLine = true
                )
                OutlinedTextField(
                    value = note, onValueChange = { note = it }, label = { Text(stringResource(R.string.profile_note)) },
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
                    note = note.trim(),
                )
                onSave(p)
            }) { Text(stringResource(R.string.save)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } }
    )
}
