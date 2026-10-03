package com.wuko233.ssh2proxy

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.res.Configuration
import java.util.Locale

/**
 * Applies the user-selected language by wrapping the base context. The value
 * "system" keeps the system locale; otherwise the app is forced to that locale.
 */
object LocaleHelper {
    const val SYSTEM = "system"
    const val ZH = "zh"
    const val EN = "en"

    fun wrap(base: Context): Context {
        val locale = when (SettingsStore.language(base)) {
            ZH -> Locale.SIMPLIFIED_CHINESE
            EN -> Locale.ENGLISH
            else -> return base
        }
        Locale.setDefault(locale)
        val config = Configuration(base.resources.configuration)
        config.setLocale(locale)
        return base.createConfigurationContext(config)
    }
}

/** Finds the hosting Activity through arbitrary ContextWrapper layers. */
tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
