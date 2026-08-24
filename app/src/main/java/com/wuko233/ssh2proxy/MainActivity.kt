package com.wuko233.ssh2proxy

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.json.JSONObject

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { MaterialTheme { App() } }
    }

    @Composable
    fun App() {
        var connected by remember { mutableStateOf(false) }
        val profile = ProfileStore.load(this@MainActivity).firstOrNull()

        fun doConnect() {
            val p = profile ?: return
            val auth = JSONObject().put("type", "password").put("password", p.password)
            val cfg = JSONObject().put("host", p.host).put("port", p.port)
                .put("username", p.username).put("auth", auth).toString()
            if (NativeBridge.connect(cfg) == 0) {
                startService(Intent(this@MainActivity, SshVpnService::class.java))
                connected = true
            }
        }

        val launcher = rememberLauncherForActivityResult(
            ActivityResultContracts.StartActivityForResult()
        ) { result ->
            if (result.resultCode == Activity.RESULT_OK) doConnect()
        }

        Column(Modifier.padding(16.dp)) {
            Text("SSH2Proxy", style = MaterialTheme.typography.headlineMedium)
            Spacer(Modifier.height(8.dp))
            if (profile == null) {
                Text("未配置")
                Spacer(Modifier.height(8.dp))
                Button(onClick = {}, enabled = false) { Text("连接") }
            } else {
                Text("Host: ${profile.host}")
                Spacer(Modifier.height(8.dp))
                Button(onClick = {
                    if (connected) {
                        NativeBridge.disconnect()
                        stopService(Intent(this@MainActivity, SshVpnService::class.java))
                        connected = false
                    } else {
                        val intent = VpnService.prepare(this@MainActivity)
                        if (intent != null) launcher.launch(intent) else doConnect()
                    }
                }) { Text(if (connected) "断开" else "连接") }
            }
        }
    }
}