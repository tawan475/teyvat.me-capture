// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import org.json.JSONArray
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

data class PlanItem(val name: String, val missing: Int?)

data class PlanCard(
    val name: String,
    val detail: String,
    val runs: Int?,
    val resin: Int?,
    val upperBound: Boolean,
    val items: List<PlanItem>,
    val who: List<String>,
)

data class PlanSection(val label: String, val resin: String, val cards: List<PlanCard>)

/**
 * The farm plan the tracker's Planner last published (`GET /irminsul/today`),
 * as the native side read it (`teyvatme_core::link::TodayDigest`, every
 * field present). Its text is already in the Planner's language.
 */
data class Plan(val account: String?, val at: Long, val weekday: Int?, val sections: List<PlanSection>) {
    companion object {
        fun parse(o: JSONObject): Plan {
            fun <T> JSONArray?.map(f: (JSONObject) -> T): List<T> =
                if (this == null) emptyList() else List(length()) { f(optJSONObject(it) ?: JSONObject()) }
            fun JSONObject.int(key: String): Int? = if (isNull(key) || !has(key)) null else optInt(key)
            return Plan(
                account = o.optJSONObject("account")?.optString("name")?.takeIf { it.isNotEmpty() },
                at = o.optLong("at"),
                weekday = o.int("weekday"),
                sections = o.optJSONArray("sections").map { s ->
                    PlanSection(
                        s.optString("label"), s.optString("resin"),
                        s.optJSONArray("cards").map { c ->
                            PlanCard(
                                c.optString("name"), c.optString("detail"), c.int("runs"), c.int("resin"),
                                c.optBoolean("upperBound"),
                                c.optJSONArray("items").map { i -> PlanItem(i.optString("name"), i.int("missing")) },
                                c.optJSONArray("who")?.let { list -> List(list.length()) { list.optString(it) } } ?: emptyList(),
                            )
                        },
                    )
                },
            )
        }
    }
}

/** What the Today screen shows from the tracker. */
sealed class PlanView {
    /** Not linked, or not read yet: nothing. */
    data object None : PlanView()
    /** Linked, nothing published: the hint to open the Planner. */
    data object NoPlan : PlanView()
    data class Ready(val plan: Plan) : PlanView()
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
