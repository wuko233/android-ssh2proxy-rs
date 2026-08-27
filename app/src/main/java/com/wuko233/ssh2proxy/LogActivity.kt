package com.wuko233.ssh2proxy

import android.app.Activity
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import org.json.JSONArray

class LogActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { LogScreen() }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LogScreen() {
    MaterialTheme(colorScheme = lightColorScheme()) {
        val ctx = LocalContext.current
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

        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text("运行日志", fontWeight = FontWeight.Bold) },
                    navigationIcon = {
                        TextButton(onClick = { (ctx as? Activity)?.finish() }) { Text("返回") }
                    },
                    actions = {
                        TextButton(onClick = { lines = emptyList() }) { Text("清空") }
                    }
                )
            }
        ) { padding ->
            val listState = rememberLazyListState()
            LaunchedEffect(lines.size) {
                if (lines.isNotEmpty()) listState.scrollToItem(lines.size - 1)
            }
            Box(Modifier.padding(padding).fillMaxSize()) {
                if (lines.isEmpty()) {
                    Text("暂无日志", Modifier.align(Alignment.Center), style = MaterialTheme.typography.bodyLarge)
                } else {
                    SelectionContainer {
                        LazyColumn(Modifier.fillMaxSize(), state = listState) {
                            items(lines) { line ->
                                Text(
                                    line,
                                    Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 2.dp),
                                    style = MaterialTheme.typography.bodySmall
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}