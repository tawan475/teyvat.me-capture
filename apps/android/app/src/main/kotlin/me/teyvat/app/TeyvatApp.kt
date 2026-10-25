// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.app.Application
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

/** The process: native logging and decoder, settings, the capture state. */
class TeyvatApp : Application() {
    lateinit var prefs: Prefs
        private set
    lateinit var capture: Capture
        private set
    private lateinit var autoExport: AutoExporter

    override fun onCreate() {
        super.onCreate()
        instance = this
        NativeBridge.init(filesDir.path)
        prefs = Prefs(this)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
        autoExport = AutoExporter(this, prefs, scope)
        capture = Capture(scope) { summary -> autoExport.tick(summary) }
    }

    companion object {
        lateinit var instance: TeyvatApp
            private set
    }
}
