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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
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
        var profiles by remember { mutableStateOf(ProfileStore.load(ctx)) }
        var selectedId by remember { mutableStateOf(profiles.firstOrNull()?.id) }
        var connected by remember { mutableStateOf(false) }
        var status by remember { mutableStateOf("") }
        var editing by remember { mutableStateOf<Profile?>(null) }
        var showAdd by remember { mutableStateOf(false) }

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
        }

        Scaffold(
            topBar = { TopAppBar(title = { Text("SSH2Proxy", fontWeight = FontWeight.Bold) }) },
            floatingActionButton = {
                FloatingActionButton(onClick = { showAdd = true }) { Text("＋", style = MaterialTheme.typography.titleLarge) }
            }
        ) { padding ->
            Column(Modifier.padding(padding).fillMaxSize().padding(16.dp)) {
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
                    Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                        Text("还没有 SSH 配置\n点右下角 ＋ 添加", style = MaterialTheme.typography.bodyLarge)
                    }
                } else {
                    LazyColumn(Modifier.weight(1f)) {
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

                Spacer(Modifier.height(12.dp))
                Button(
                    onClick = { if (connected) disconnect() else connect() },
                    enabled = connected || selected != null,
                    modifier = Modifier.fillMaxWidth().height(52.dp)
                ) {
                    Text(if (connected) "断开连接" else "连接", fontWeight = FontWeight.Bold)
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