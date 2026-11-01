// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import org.json.JSONObject

/**
 * This device's alert settings on the tracker, as far as the resin alert
 * goes (the presence answer's `alerts`; the rest is the tracker's).
 */
data class TrackerAlerts(val resin: Int?, val resinFull: Boolean) {
    /** The amounts to alert at, lowest first, without repeats (`teyvatme_core::link::Alerts::resin_targets`). */
    fun targets(): List<Int> =
        listOfNotNull(resin?.coerceIn(1, Today.RESIN_MAX), Today.RESIN_MAX.takeIf { resinFull }).distinct()

    fun toJson(): String = JSONObject().put("resin", resin ?: JSONObject.NULL).put("resinFull", resinFull).toString()

    companion object {
        fun parse(o: JSONObject?): TrackerAlerts? = o?.let {
            TrackerAlerts(if (it.isNull("resin")) null else it.optInt("resin"), it.optBoolean("resinFull"))
        }
    }
}

/**
 * What the tracker hears about the game (the desktop's `presence_state`):
 * `stopped` with capture off or no game connection, `decoding` once the
 * account's data decrypts, `running` otherwise.
 */
fun presenceState(vpn: VpnState, summary: Summary): String = when {
    vpn == VpnState.Off -> "stopped"
    summary.phase == "decrypting" || summary.phase == "done" -> "decoding"
    summary.phase == "waiting" -> "stopped"
    else -> "running"
}
