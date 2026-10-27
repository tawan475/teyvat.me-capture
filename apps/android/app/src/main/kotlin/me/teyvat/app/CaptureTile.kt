// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.drawable.Icon
import android.net.VpnService
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.util.Log

/**
 * The Quick Settings tile: off, a tap starts capture in the background; on,
 * a tap stops capture. It opens nothing (Genshin is the player's to open).
 *
 * Only the first start goes through the app's screen: Android shows the VPN
 * prompt only from one. Once that was answered (and setup is done) the tile
 * starts the service itself, like a VPN app's tile; if Android refuses that
 * start, the app's own Start capture runs instead.
 */
class CaptureTile : TileService() {
    override fun onStartListening() {
        super.onStartListening()
        refresh()
    }

    override fun onClick() {
        super.onClick()
        val app = application as TeyvatApp
        if (app.capture.vpn.value != VpnState.Off) {
            GameVpnService.stop(this)
            return
        }
        if (startInBackground(app)) return
        collapseInto(
            Intent(this, MainActivity::class.java)
                .setAction(MainActivity.ACTION_START_CAPTURE)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }

    /** Starts capture without the app's screen; `false` when only the app can (first VPN prompt, setup, refused). */
    private fun startInBackground(app: TeyvatApp): Boolean {
        if (!app.prefs.value.onboarded || VpnService.prepare(this) != null) return false
        app.capture.setVpn(VpnState.Starting)
        return runCatching { GameVpnService.start(this) }
            .onFailure {
                Log.w(TAG, "tile start refused, opening the app", it)
                app.capture.setVpn(VpnState.Off)
            }
            .isSuccess
    }

    @SuppressLint("StartActivityAndCollapseDeprecated") // the Intent form only below Android 14
    private fun collapseInto(intent: Intent) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startActivityAndCollapse(
                PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT),
            )
        } else {
            @Suppress("DEPRECATION")
            startActivityAndCollapse(intent)
        }
    }

    private fun refresh() {
        val tile = qsTile ?: return
        val on = (application as TeyvatApp).capture.vpn.value != VpnState.Off
        tile.state = if (on) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        tile.icon = Icon.createWithResource(this, R.drawable.ic_capture)
        tile.label = getString(R.string.launcher_name)
        tile.subtitle = getString(if (on) R.string.stop_capture else R.string.home_start_capture)
        tile.updateTile()
    }

    companion object {
        private const val TAG = "teyvatme"

        /** Ask Android to redraw the tile (capture started or stopped elsewhere). */
        fun update(context: Context) {
            runCatching { requestListeningState(context, ComponentName(context, CaptureTile::class.java)) }
        }
    }
}
