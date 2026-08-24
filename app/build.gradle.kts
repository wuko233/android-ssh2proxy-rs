plugins {
    id("com.android.application")
}

android {
    namespace = "com.wuko233.ssh2proxy"
    compileSdk = 37
    defaultConfig {
        applicationId = "com.wuko233.ssh2proxy"
        minSdk = 24
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
}