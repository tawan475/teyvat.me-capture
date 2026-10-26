// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings as AndroidSettings

/**
 * Android's battery optimisation, and the phone makers' savers on top of
 * it, can pause a background app even with a foreground service: the
 * capture keeps running in the VPN, but the exports and uploads that follow
 * it wait until the app is opened. Unrestricted is what lets them go on
 * while the game is in front.
 */
object Battery {
    const val DONT_KILL_MY_APP = "https://dontkillmyapp.com/"

    fun unrestricted(context: Context): Boolean =
        context.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(context.packageName)

    /** Android's "Allow app to always run in background?" dialog; its list as a fallback. */
    @SuppressLint("BatteryLife")
    fun ask(context: Context) {
        val direct = Intent(AndroidSettings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${context.packageName}"))
        runCatching { context.startActivity(direct) }.onFailure {
            runCatching { context.startActivity(Intent(AndroidSettings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)) }
        }
    }
}
