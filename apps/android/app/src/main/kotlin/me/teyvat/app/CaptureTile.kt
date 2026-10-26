// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.drawable.Icon
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService

/**
 * The Quick Settings tile: off, a tap starts capture and opens Genshin once
 * the VPN is up (the game must connect after it, for the login's key); on, a
 * tap stops capture.
 *
 * Starting goes through the app, as its own Start capture does: Android
 * shows the VPN and notification prompts only from a screen, and a service
 * may not start a foreground service from a tile on every version.
 */
class CaptureTile : TileService() {
    override fun onStartListening() {
        super.onStartListening()
        refresh()
    }

    override fun onClick() {
        super.onClick()
        if ((application as TeyvatApp).capture.vpn.value != VpnState.Off) {
            GameVpnService.stop(this)
            return
        }
        val start = Intent(this, MainActivity::class.java)
            .setAction(MainActivity.ACTION_START_CAPTURE)
            .putExtra(MainActivity.EXTRA_OPEN_GAME, true)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startActivityAndCollapse(
                PendingIntent.getActivity(this, 0, start, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT),
            )
        } else {
            @Suppress("DEPRECATION")
            startActivityAndCollapse(start)
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
        /** Ask Android to redraw the tile (capture started or stopped elsewhere). */
        fun update(context: Context) {
            runCatching { requestListeningState(context, ComponentName(context, CaptureTile::class.java)) }
        }
    }
}
