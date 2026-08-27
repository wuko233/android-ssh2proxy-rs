package com.wuko233.ssh2proxy

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import org.json.JSONObject
import java.util.Locale

class SshVpnService : VpnService() {
    private val handler = Handler(Looper.getMainLooper())
    private var lastUp = 0L
    private var lastDown = 0L
    private var lastTime = 0L

    private val updateRunnable = object : Runnable {
        override fun run() {
            updateNotification()
            handler.postDelayed(this, 1000)
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startForeground(1, buildNotification("代理运行中"))
        val fd = establish() ?: return START_NOT_STICKY
        NativeBridge.setTunFd(fd.detachFd())
        lastUp = 0L
        lastDown = 0L
        lastTime = System.currentTimeMillis()
        handler.post(updateRunnable)
        return START_STICKY
    }

    override fun onDestroy() {
        handler.removeCallbacks(updateRunnable)
        NativeBridge.closeTun()
        super.onDestroy()
    }

    private fun establish(): ParcelFileDescriptor? {
        return Builder()
            .setMtu(1500)
            .addAddress("10.0.0.2", 24)
            .addRoute("0.0.0.0", 0)
            .addDnsServer("10.0.0.1")
            .addDisallowedApplication(packageName)
            .setSession("SSH2Proxy")
            .establish()
    }

    private fun updateNotification() {
        var up = 0L
        var down = 0L
        try {
            val o = JSONObject(NativeBridge.getStats())
            up = o.optLong("up")
            down = o.optLong("down")
        } catch (_: Exception) {
        }
        val now = System.currentTimeMillis()
        val dt = (now - lastTime) / 1000.0
        val upRate = if (dt > 0) (up - lastUp) / dt else 0.0
        val downRate = if (dt > 0) (down - lastDown) / dt else 0.0
        lastUp = up
        lastDown = down
        lastTime = now
        val text = "↑ ${formatRate(upRate)}  ↓ ${formatRate(downRate)}"
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(1, buildNotification(text))
    }

    private fun buildNotification(text: String): Notification {
        val pi = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE
        )
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val ch = NotificationChannel("vpn", "VPN", NotificationManager.IMPORTANCE_LOW)
            getSystemService(NotificationManager::class.java).createNotificationChannel(ch)
            Notification.Builder(this, "vpn")
                .setContentTitle("SSH2Proxy")
                .setContentText(text)
                .setSmallIcon(android.R.drawable.ic_lock_lock)
                .setContentIntent(pi)
                .setOngoing(true)
                .build()
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
                .setContentTitle("SSH2Proxy")
                .setContentText(text)
                .setSmallIcon(android.R.drawable.ic_lock_lock)
                .setContentIntent(pi)
                .setOngoing(true)
                .build()
        }
    }

    private fun formatRate(bps: Double): String {
        val kb = 1024.0
        val mb = kb * 1024
        val gb = mb * 1024
        return when {
            bps >= gb -> String.format(Locale.US, "%.2f GB/s", bps / gb)
            bps >= mb -> String.format(Locale.US, "%.2f MB/s", bps / mb)
            bps >= kb -> String.format(Locale.US, "%.1f KB/s", bps / kb)
            else -> String.format(Locale.US, "%.0f B/s", bps)
        }
    }
}