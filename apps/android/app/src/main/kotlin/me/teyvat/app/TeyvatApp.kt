// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.app.Application
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

/** The process: native logging and decoder, settings, the capture state. */
class TeyvatApp : Application() {
    lateinit var prefs: Prefs
        private set
    lateinit var capture: Capture
        private set
    lateinit var tracker: Tracker
        private set
    lateinit var updater: Updater
        private set
    private lateinit var autoExport: AutoExporter

    override fun onCreate() {
        super.onCreate()
        instance = this
        keepCrashes()
        NativeBridge.init(filesDir.path)
        prefs = Prefs(this)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
        autoExport = AutoExporter(this, prefs, scope)
        tracker = Tracker(this, prefs, scope)
        capture = Capture(scope) { summary ->
            autoExport.tick(summary)
            tracker.tick(summary)
        }
        updater = Updater(this, prefs, scope) { capture.vpn.value != VpnState.Off }
    }

    /**
     * An uncaught exception is written to files/crash.txt (Share log sends
     * it) before Android's own handler ends the app. Native panics go to
     * latest.log (logging.rs).
     */
    private fun keepCrashes() {
        val android = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            runCatching {
                val at = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.ROOT).format(Date())
                File(filesDir, "crash.txt").writeText(
                    "Teyvat.me Capture ${BuildConfig.VERSION_NAME} crashed at $at on thread ${thread.name}\n\n" +
                        error.stackTraceToString(),
                )
            }
            android?.uncaughtException(thread, error)
        }
    }

    companion object {
        lateinit var instance: TeyvatApp
            private set
    }
}
