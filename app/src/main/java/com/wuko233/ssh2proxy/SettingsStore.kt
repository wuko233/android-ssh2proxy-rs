package com.wuko233.ssh2proxy

import android.content.Context

object SettingsStore {
    private const val PREFS = "settings"
    private const val KEY_UDP = "udp_enabled"
    private const val KEY_LOG_DEBUG = "log_debug"

    private fun prefs(ctx: Context) =
        ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun udpEnabled(ctx: Context): Boolean =
        prefs(ctx).getBoolean(KEY_UDP, true)

    fun setUdpEnabled(ctx: Context, value: Boolean) =
        prefs(ctx).edit().putBoolean(KEY_UDP, value).apply()

    fun logDebug(ctx: Context): Boolean =
        prefs(ctx).getBoolean(KEY_LOG_DEBUG, false)

    fun setLogDebug(ctx: Context, value: Boolean) =
        prefs(ctx).edit().putBoolean(KEY_LOG_DEBUG, value).apply()
}