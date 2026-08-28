package com.wuko233.ssh2proxy

import android.content.Context

object SettingsStore {
    private const val PREFS = "settings"
    private const val KEY_UDP = "udp_enabled"
    private const val KEY_LOG_DEBUG = "log_debug"
    private const val KEY_LOCAL_PROXY = "local_proxy_mode"
    private const val KEY_LAN_SHARE = "lan_share"
    private const val KEY_SOCKS_PORT = "socks_port"
    private const val KEY_HTTP_PORT = "http_port"
    private const val KEY_TEST_DOMAIN = "test_domain"
    private const val KEY_TEST_TARGET = "test_target"

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

    fun localProxyMode(ctx: Context): Boolean =
        prefs(ctx).getBoolean(KEY_LOCAL_PROXY, false)

    fun setLocalProxyMode(ctx: Context, value: Boolean) =
        prefs(ctx).edit().putBoolean(KEY_LOCAL_PROXY, value).apply()

    fun lanShare(ctx: Context): Boolean =
        prefs(ctx).getBoolean(KEY_LAN_SHARE, false)

    fun setLanShare(ctx: Context, value: Boolean) =
        prefs(ctx).edit().putBoolean(KEY_LAN_SHARE, value).apply()

    fun socksPort(ctx: Context): Int =
        prefs(ctx).getInt(KEY_SOCKS_PORT, 1080)

    fun setSocksPort(ctx: Context, value: Int) =
        prefs(ctx).edit().putInt(KEY_SOCKS_PORT, value).apply()

    fun httpPort(ctx: Context): Int =
        prefs(ctx).getInt(KEY_HTTP_PORT, 8888)

    fun setHttpPort(ctx: Context, value: Int) =
        prefs(ctx).edit().putInt(KEY_HTTP_PORT, value).apply()

    fun testDomain(ctx: Context): String =
        prefs(ctx).getString(KEY_TEST_DOMAIN, "www.baidu.com") ?: "www.baidu.com"

    fun setTestDomain(ctx: Context, value: String) =
        prefs(ctx).edit().putString(KEY_TEST_DOMAIN, value).apply()

    fun testTarget(ctx: Context): String =
        prefs(ctx).getString(KEY_TEST_TARGET, "223.5.5.5:53") ?: "223.5.5.5:53"

    fun setTestTarget(ctx: Context, value: String) =
        prefs(ctx).edit().putString(KEY_TEST_TARGET, value).apply()
}