package com.wuko233.ssh2proxy

object NativeBridge {
    init {
        System.loadLibrary("ssh2proxy")
    }
    external fun connect(configJson: String): Int
    external fun setTunFd(fd: Int)
    external fun closeTun()
    external fun disconnect()
    external fun getStats(): String
    external fun pollEvents(): String
    external fun runConnectivityTest(domain: String): String
    external fun runLatencyTest(target: String): String
    external fun setLogLevel(debug: Boolean)
}
