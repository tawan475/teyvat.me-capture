// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** The hero's title (also the notification's title). */
fun Hero.title(): Int = when (this) {
    Hero.CaptureOff -> R.string.home_capture_off_title
    Hero.Starting -> R.string.home_starting_title
    Hero.Waiting -> R.string.home_waiting_title
    Hero.GameDetected, Hero.GameAlreadyOpen -> R.string.home_detected_title
    Hero.LoginSeen -> R.string.home_login_seen_title
    Hero.Decrypting -> R.string.home_decrypting_title
    Hero.MissedAlreadyRunning, Hero.MissedCaptureStopped -> R.string.home_missed_title
    Hero.KeyLost -> R.string.home_key_lost_title
}

/** `HH:mm`, 24 h. */
fun hhmm(at: Long): String = SimpleDateFormat("HH:mm", Locale.ROOT).format(Date(at))

/** The hero's line under the title. */
fun Hero.line(context: Context, summary: Summary): String = when (this) {
    Hero.CaptureOff -> context.getString(R.string.home_capture_off_line)
    Hero.Starting -> context.getString(R.string.home_starting_line)
    Hero.Waiting -> context.getString(R.string.home_waiting_line)
    Hero.GameDetected -> context.getString(R.string.home_detected_line)
    Hero.GameAlreadyOpen -> context.getString(R.string.home_already_open_line)
    Hero.LoginSeen -> context.getString(R.string.home_login_seen_line)
    Hero.Decrypting -> context.getString(R.string.home_decrypting_line, summary.keyAt?.let(::hhmm) ?: "")
    Hero.MissedAlreadyRunning -> context.getString(R.string.home_missed_running_line)
    Hero.MissedCaptureStopped -> context.getString(R.string.home_missed_stopped_line)
    Hero.KeyLost -> context.getString(R.string.home_key_lost_line)
}

/** The footer's activity line: the newest event, "HH:mm · what". */
fun activityLine(summary: Summary): ActivityEvent? = summary.events.lastOrNull()

fun ActivityEvent.line() = "${hhmm(at)} · $text"

/** The decoder's activity lines (written in English) in the app's language. */
fun nativeEventText(text: String): String {
    val context = runCatching { TeyvatApp.instance }.getOrNull() ?: return text
    val id = when (text) {
        "Game connected" -> R.string.event_game_connected
        "Login seen" -> R.string.home_login_seen_title
        "Session key found" -> R.string.event_key_found
        "Session key not found" -> R.string.event_key_lost
        "Account data captured" -> R.string.event_data_captured
        else -> null
    }
    if (id != null) return context.getString(id)
    val incomplete = "Export incomplete: "
    if (text.startsWith(incomplete)) return context.getString(R.string.event_export_incomplete, text.removePrefix(incomplete))
    return text
}

/** Thousands grouped: `1,684`. */
fun grouped(n: Int): String = String.format(Locale.US, "%,d", n)

/** The short form of the embedded game data (its dump commit). */
fun gameDataLabel(summary: Summary) = summary.gameData?.take(7) ?: "unknown"

object Links {
    const val GAME_PACKAGE = GameVpnService.GAME_PACKAGE
    const val SOURCE = "https://github.com/tawan475/teyvat.me-capture"
    const val ISSUES = "https://github.com/tawan475/teyvat.me-capture/issues/new"
    const val DEFAULT_TRACKER_API = "https://teyvat.me/api"
    const val STARDB_ACHIEVEMENTS = "https://stardb.gg/import"
    const val SEELIE_ACHIEVEMENTS = "https://seelie.me/achievements"
    const val PAIMON_WISH_IMPORT = "https://paimon.moe/wish/import"
    const val STARDB_WISH_IMPORT = "https://stardb.gg/en/genshin/wish-import"

    /** The tracker's API address (Settings › Tracker › Advanced; [Tracker.setApi]). */
    val trackerApi: String
        get() = runCatching {
            TeyvatApp.instance.getSharedPreferences("tracker", Context.MODE_PRIVATE).getString("api", null)
        }.getOrNull() ?: DEFAULT_TRACKER_API

    /** The tracker's site: the API address without `/api`. */
    val trackerSite: String get() = trackerApi.removeSuffix("/api")
    val trackerHost: String get() = Uri.parse(trackerApi).host ?: trackerApi

    fun gameLaunch(context: Context): Intent? = context.packageManager.getLaunchIntentForPackage(GAME_PACKAGE)

    /** Android has no API to stop another app: its App info page has Force stop. */
    fun gameAppInfo(): Intent = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$GAME_PACKAGE"))
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
}
