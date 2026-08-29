package com.wuko233.ssh2proxy

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

object BackupStore {
    fun exportJson(ctx: Context): String {
        val root = JSONObject()
        root.put("version", 1)
        val profiles = JSONArray()
        ProfileStore.load(ctx).forEach { p ->
            profiles.put(
                JSONObject()
                    .put("host", p.host)
                    .put("port", p.port)
                    .put("username", p.username)
                    .put("password", p.password)
                    .put("dnsServer", p.dnsServer)
                    .put("note", p.note)
            )
        }
        root.put("profiles", profiles)
        val settings = JSONObject()
            .put("udp_enabled", SettingsStore.udpEnabled(ctx))
            .put("log_debug", SettingsStore.logDebug(ctx))
            .put("local_proxy_mode", SettingsStore.localProxyMode(ctx))
            .put("lan_share", SettingsStore.lanShare(ctx))
            .put("socks_port", SettingsStore.socksPort(ctx))
            .put("http_port", SettingsStore.httpPort(ctx))
            .put("test_domain", SettingsStore.testDomain(ctx))
            .put("test_target", SettingsStore.testTarget(ctx))
        root.put("settings", settings)
        return root.toString(2)
    }

    fun importJson(ctx: Context, json: String): Boolean {
        return try {
            val root = JSONObject(json)
            val profiles = root.optJSONArray("profiles")
            if (profiles != null) {
                val list = (0 until profiles.length()).map { i ->
                    val o = profiles.getJSONObject(i)
                    Profile(
                        id = UUID.randomUUID().toString(),
                        host = o.getString("host"),
                        port = o.optInt("port", 22),
                        username = o.optString("username", "root"),
                        password = o.optString("password", ""),
                        dnsServer = o.optString("dnsServer", "8.8.8.8"),
                        note = o.optString("note", ""),
                    )
                }
                ProfileStore.save(ctx, list)
            }
            val settings = root.optJSONObject("settings")
            if (settings != null) {
                SettingsStore.setUdpEnabled(ctx, settings.optBoolean("udp_enabled", true))
                SettingsStore.setLogDebug(ctx, settings.optBoolean("log_debug", false))
                SettingsStore.setLocalProxyMode(ctx, settings.optBoolean("local_proxy_mode", false))
                SettingsStore.setLanShare(ctx, settings.optBoolean("lan_share", false))
                SettingsStore.setSocksPort(ctx, settings.optInt("socks_port", 1080))
                SettingsStore.setHttpPort(ctx, settings.optInt("http_port", 8888))
                SettingsStore.setTestDomain(ctx, settings.optString("test_domain", "www.baidu.com"))
                SettingsStore.setTestTarget(ctx, settings.optString("test_target", "223.5.5.5:53"))
            }
            true
        } catch (_: Exception) {
            false
        }
    }
}