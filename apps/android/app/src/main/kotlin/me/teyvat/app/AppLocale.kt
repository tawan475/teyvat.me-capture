// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.app.Activity
import android.app.LocaleManager
import android.content.Context
import android.content.res.Configuration
import android.content.res.Resources
import android.os.Build
import android.os.LocaleList
import java.util.Locale

/**
 * The app's language, picked in Settings like the desktop's: every language
 * the game ships, or the system's. Android 13+ keeps it per app (and shows
 * it in system settings) for every part of the app; older versions keep it
 * here and apply it to the activity ([wrap]) and to the app's own resources
 * ([applyToApp]), which the capture service, its notifications, the resin
 * alert and the tile read.
 */
object AppLocale {
    /** The desktop's list (`i18n.rs`), with each language's own name. */
    val LANGUAGES = listOf(
        "en" to "English", "zh-CN" to "简体中文", "zh-TW" to "繁體中文", "ja" to "日本語", "ko" to "한국어",
        "es" to "Español", "fr" to "Français", "ru" to "Русский", "th" to "ภาษาไทย", "vi" to "Tiếng Việt",
        "de" to "Deutsch", "id" to "Bahasa Indonesia", "pt" to "Português", "it" to "Italiano", "tr" to "Türkçe",
    )

    private fun store(context: Context) = context.getSharedPreferences("settings", Context.MODE_PRIVATE)

    /** The picked language's tag, or null for the system's. */
    fun current(context: Context): String? =
        if (Build.VERSION.SDK_INT >= 33) {
            context.getSystemService(LocaleManager::class.java).applicationLocales.takeIf { !it.isEmpty }?.get(0)?.let(::tagOf)
        } else {
            store(context).getString("language", null)
        }

    /** The tag as [LANGUAGES] names it (Android says `in` for Indonesian, `zh-Hans-CN`…). */
    private fun tagOf(locale: Locale): String? {
        val language = if (locale.language == "in") "id" else locale.language
        return when (language) {
            "zh" -> when {
                locale.script == "Hant" -> "zh-TW"
                locale.script == "Hans" -> "zh-CN"
                locale.country in listOf("TW", "HK", "MO") -> "zh-TW"
                else -> "zh-CN"
            }
            else -> LANGUAGES.firstOrNull { it.first == language }?.first
        }
    }

    fun set(activity: Activity, tag: String?) {
        if (Build.VERSION.SDK_INT >= 33) {
            activity.getSystemService(LocaleManager::class.java).applicationLocales =
                if (tag == null) LocaleList.getEmptyLocaleList() else LocaleList.forLanguageTags(tag)
        } else {
            store(activity).edit().putString("language", tag).apply()
            applyToApp(activity.applicationContext)
            activity.recreate()
        }
    }

    /** Before Android 13: the activity's resources in the picked language. */
    fun wrap(base: Context): Context {
        if (Build.VERSION.SDK_INT >= 33) return base
        val tag = store(base).getString("language", null) ?: return base
        val config = Configuration(base.resources.configuration)
        config.setLocales(LocaleList.forLanguageTags(tag))
        return base.createConfigurationContext(config)
    }

    /**
     * Before Android 13: the picked language on the app's shared resources,
     * which services, receivers and the tile read (they never pass through
     * [wrap]); a system configuration change resets them, so the app calls
     * this again then. Back to the system's languages for no pick.
     */
    @Suppress("DEPRECATION")
    fun applyToApp(app: Context) {
        if (Build.VERSION.SDK_INT >= 33) return
        val tag = store(app).getString("language", null)
        val resources = app.resources
        val config = Configuration(resources.configuration)
        config.setLocales(if (tag == null) Resources.getSystem().configuration.locales else LocaleList.forLanguageTags(tag))
        resources.updateConfiguration(config, resources.displayMetrics)
    }
}
