// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Insets
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Window
import android.view.WindowInsets
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView

/** Phase 0 spike screen: Start/Stop and the forwarder's counters. */
class MainActivity : Activity() {
    private lateinit var status: TextView
    private val ticker = Handler(Looper.getMainLooper())
    private val tick = object : Runnable {
        override fun run() {
            val stats = NativeBridge.stats()
            status.text = if (stats.isEmpty()) "Stopped\n${GameVpnService.lastPcap ?: ""}"
            else "Running\n${GameVpnService.lastPcap}\n\n${stats.replace(' ', '\n')}"
            ticker.postDelayed(this, 1000)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Target SDK 35+ draws edge to edge, and the framework's action bar
        // then sits over the top of the content (it covered most of Start VPN,
        // so taps there never reached it). No action bar; pad by the bars.
        requestWindowFeature(Window.FEATURE_NO_TITLE)
        val pad = (16 * resources.displayMetrics.density).toInt()
        status = TextView(this).apply { textSize = 16f }
        val start = Button(this).apply {
            text = "Start VPN"
            setOnClickListener { requestVpn() }
        }
        val stop = Button(this).apply {
            text = "Stop VPN"
            setOnClickListener {
                startService(Intent(this@MainActivity, GameVpnService::class.java).setAction(GameVpnService.ACTION_STOP))
            }
        }
        setContentView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(start)
            addView(stop)
            addView(status)
            setOnApplyWindowInsetsListener { view, insets ->
                val bars = systemBars(insets)
                view.setPadding(pad + bars.left, pad + bars.top, pad + bars.right, pad + bars.bottom)
                insets
            }
        })
    }

    private fun systemBars(insets: WindowInsets): Insets =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
        } else {
            @Suppress("DEPRECATION")
            insets.systemWindowInsets
        }


    /** Notifications first (Android 13+), for the service's Stop notification; refusing still starts. */
    private fun requestVpn() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQUEST_NOTIFICATIONS)
            return
        }
        requestConsent()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        Log.i("teyvatme", "notification permission: ${grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED}")
        if (requestCode == REQUEST_NOTIFICATIONS) requestConsent()
    }

    private fun requestConsent() {
        val consent = VpnService.prepare(this)
        Log.i("teyvatme", "Start tapped, consent needed: ${consent != null}")
        if (consent != null) startActivityForResult(consent, REQUEST_VPN) else startVpn()
    }

    @Deprecated("Activity result API is androidx; the spike has no dependencies")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        Log.i("teyvatme", "VPN consent result: $resultCode")
        if (requestCode == REQUEST_VPN && resultCode == RESULT_OK) startVpn()
    }

    private fun startVpn() {
        val started = startForegroundService(Intent(this, GameVpnService::class.java))
        Log.i("teyvatme", "startForegroundService -> $started")
    }

    override fun onResume() {
        super.onResume()
        ticker.post(tick)
    }

    override fun onPause() {
        ticker.removeCallbacks(tick)
        super.onPause()
    }

    private companion object {
        const val REQUEST_VPN = 1
        const val REQUEST_NOTIFICATIONS = 2
    }
}
