// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package io.github.tawan475.logbook

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.MotionEvent
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView

/** Phase 0 spike screen: Start/Stop and the forwarder's counters. */
class MainActivity : Activity() {
    private lateinit var status: TextView
    private lateinit var buttons: List<Button>
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
        buttons = listOf(start, stop)
        setContentView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad * 3, pad, pad)
            addView(start)
            addView(stop)
            addView(status)
        })
    }

    /**
     * Spike diagnostic for taps that never click on the Galaxy A54: every
     * touch's action, screen position, tool, flags (0x1 = window obscured)
     * and the button under it, if any.
     */
    override fun dispatchTouchEvent(ev: MotionEvent): Boolean {
        val handled = super.dispatchTouchEvent(ev)
        if (ev.actionMasked != MotionEvent.ACTION_MOVE) {
            val under = buttons.firstOrNull { it.contains(ev.rawX, ev.rawY) }?.text ?: "-"
            Log.i(
                "logbook",
                "touch ${MotionEvent.actionToString(ev.action)} at (${ev.rawX.toInt()}, ${ev.rawY.toInt()}) " +
                    "tool=${ev.getToolType(0)} flags=0x${ev.flags.toString(16)} on=$under handled=$handled",
            )
        }
        return handled
    }

    private fun View.contains(x: Float, y: Float): Boolean {
        val at = IntArray(2).also(::getLocationOnScreen)
        return x >= at[0] && x < at[0] + width && y >= at[1] && y < at[1] + height
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
        Log.i("logbook", "notification permission: ${grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED}")
        if (requestCode == REQUEST_NOTIFICATIONS) requestConsent()
    }

    private fun requestConsent() {
        val consent = VpnService.prepare(this)
        Log.i("logbook", "Start tapped, consent needed: ${consent != null}")
        if (consent != null) startActivityForResult(consent, REQUEST_VPN) else startVpn()
    }

    @Deprecated("Activity result API is androidx; the spike has no dependencies")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        Log.i("logbook", "VPN consent result: $resultCode")
        if (requestCode == REQUEST_VPN && resultCode == RESULT_OK) startVpn()
    }

    private fun startVpn() {
        val started = startForegroundService(Intent(this, GameVpnService::class.java))
        Log.i("logbook", "startForegroundService -> $started")
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
