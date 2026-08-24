package com.wuko233.ssh2proxy

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
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
        Column(Modifier.padding(16.dp)) {
            Text("SSH2Proxy", style = MaterialTheme.typography.headlineMedium)
            Spacer(Modifier.height(8.dp))
            Text("Host: ${ProfileStore.load(this@MainActivity).firstOrNull()?.host ?: "未配置"}")
            Spacer(Modifier.height(8.dp))
            Button(onClick = {
                if (connected) {
                    NativeBridge.disconnect()
                    connected = false
                } else {
                    val p = ProfileStore.load(this@MainActivity).first()
                    val auth = JSONObject().put("type", "password").put("password", p.password)
                    val cfg = JSONObject().put("host", p.host).put("port", p.port)
                        .put("username", p.username).put("auth", auth).toString()
                    if (NativeBridge.connect(cfg) == 0) {
                        val ok = VpnService.prepare(this@MainActivity) == null
                        if (ok) {
                            startService(Intent(this@MainActivity, SshVpnService::class.java))
                            connected = true
                        }
                    }
                }
            }) { Text(if (connected) "断开" else "连接") }
        }
    }
}