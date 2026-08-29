package com.wuko233.ssh2proxy

import android.content.Context
import org.json.JSONObject

data class Profile(
    val id: String,
    val host: String,
    val port: Int,
    val username: String,
    val password: String,
    val dnsServer: String = "8.8.8.8",
    val note: String = "",
)

object ProfileStore {
    private const val KEY = "profiles"
    fun load(ctx: Context): List<Profile> {
        val raw = ctx.getSharedPreferences("cfg", Context.MODE_PRIVATE).getString(KEY, "[]")!!
        val arr = org.json.JSONArray(raw)
        return (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            Profile(o.getString("id"), o.getString("host"), o.getInt("port"),
                    o.getString("username"), o.getString("password"),
                    o.optString("dnsServer", "8.8.8.8"),
                    o.optString("note", ""))
        }
    }
    fun save(ctx: Context, list: List<Profile>) {
        val arr = org.json.JSONArray()
        list.forEach { p ->
            arr.put(JSONObject().put("id", p.id).put("host", p.host).put("port", p.port)
                .put("username", p.username).put("password", p.password)
                .put("dnsServer", p.dnsServer).put("note", p.note))
        }
        ctx.getSharedPreferences("cfg", Context.MODE_PRIVATE).edit().putString(KEY, arr.toString()).apply()
    }
}