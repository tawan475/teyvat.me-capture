// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.app.Application
import android.content.res.Configuration
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

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
    private lateinit var resinAlert: ResinAlert

    override fun onCreate() {
        super.onCreate()
        instance = this
        AppLocale.applyToApp(this)
        Clock.refresh(this)
        keepCrashes()
        NativeBridge.init(filesDir.path)
        prefs = Prefs(this)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
        autoExport = AutoExporter(this, prefs, scope)
        tracker = Tracker(this, prefs, scope)
        resinAlert = ResinAlert(this, prefs) { tracker.alerts.value }
        WishTrackerBridge.tracker = tracker
        capture = Capture(scope) { vpn, summary ->
            autoExport.tick(summary)
            tracker.tick(vpn, summary)
            resinAlert.tick(summary)
        }
        // A changed resin alert reschedules at once: the capture's own reads may
        // be hours away (game closed), and the old alarm would fire at the old level.
        // The tracker's settings for this device count the same.
        scope.launch {
            prefs.settings.map { it.resinAlert to it.resinAlertAt }.distinctUntilChanged().drop(1)
                .collect { capture.refresh() }
        }
        scope.launch { tracker.alerts.drop(1).collect { capture.refresh() } }
        updater = Updater(this, prefs, scope) { capture.vpn.value != VpnState.Off }
        // The Quick Settings tile follows capture, however it was started or stopped.
        scope.launch { capture.vpn.collect { CaptureTile.update(this@TeyvatApp) } }
    }

    /** A system change (rotation, dark mode, the system's language) resets the picked language before Android 13. */
    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        AppLocale.applyToApp(this)
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
