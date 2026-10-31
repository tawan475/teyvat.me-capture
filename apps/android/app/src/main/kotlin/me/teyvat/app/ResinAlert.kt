// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * The resin alert (Settings, off by default): a notification when the
 * capture's resin clock reaches the threshold. While the tracker holds
 * alert settings for this device ([trackerAlerts]: signed in) they decide
 * instead: its amount and/or full. An alarm, so it comes with the app
 * closed too; set again whenever the clock or the setting changes
 * (TeyvatApp rereads the capture on a settings change), and never twice for
 * the same clock and amount (kept across restarts, since an alarm set in
 * the past fires at once). One alarm at a time: the next amount's.
 */
class ResinAlert(private val context: Context, private val prefs: Prefs, private val trackerAlerts: () -> TrackerAlerts?) {
    /** What this process last scheduled; unset at first, so the first tick also cancels an earlier process's alarm. */
    private var scheduled: String? = UNSET

    fun tick(summary: Summary) {
        val settings = prefs.value
        val today = summary.today
        val targets = trackerAlerts()?.targets()
            ?: listOfNotNull(settings.resinAlertAt.coerceIn(1, Today.RESIN_MAX).takeIf { settings.resinAlert })
        // The highest amount already sent on this clock.
        val clock = "${today?.resin}:${today?.resinNextAt}"
        val done = sent(context)?.takeIf { it.substringBeforeLast(':') == clock }?.substringAfterLast(':')?.toIntOrNull() ?: 0
        val next = if (today == null) null else targets.filter { it > done }.firstNotNullOfOrNull { t -> reaches(today, t)?.let { t to it } }
        val target = next?.first ?: 0
        val key = next?.let { "$clock:${it.first}" }
        if (key == scheduled) return
        scheduled = key
        val alarms = context.getSystemService(AlarmManager::class.java)
        val pending = pendingIntent(context, target, today?.resinFullAt())
        if (next == null) {
            alarms.cancel(pending)
            return
        }
        alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, next.second * 1000, pending)
        context.getSharedPreferences(STORE, Context.MODE_PRIVATE).edit().putString(PENDING, key).apply()
    }

    companion object {
        private const val UNSET = "-"
        private const val STORE = "resin_alert"
        private const val PENDING = "pending"
        private const val SENT = "sent"
        private const val EXTRA_TARGET = "target"
        private const val EXTRA_FULL_AT = "full_at"
        private const val NOTIFICATION_ID = 20
        private const val ALERTS = "alerts"

        /** When the clock reaches [target] (unix s); null when it already had that much. */
        fun reaches(today: Today, target: Int): Long? {
            val value = today.resin ?: return null
            val next = today.resinNextAt?.takeIf { it > 0 } ?: return null
            if (value >= target || target > Today.RESIN_MAX) return null
            return next + (target - value - 1) * Today.RESIN_EVERY_S
        }

        private fun sent(context: Context): String? =
            context.getSharedPreferences(STORE, Context.MODE_PRIVATE).getString(SENT, null)

        private fun pendingIntent(context: Context, target: Int, fullAt: Long?): PendingIntent {
            val intent = Intent(context, ResinAlarm::class.java)
                .putExtra(EXTRA_TARGET, target)
                .putExtra(EXTRA_FULL_AT, fullAt ?: 0L)
            return PendingIntent.getBroadcast(
                context, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        }

        /** Show the alert (the alarm's, or Settings' test). */
        fun show(context: Context, target: Int, fullAt: Long) {
            GameVpnService.createChannels(context)
            val open = PendingIntent.getActivity(
                context, 0, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE,
            )
            val nowS = System.currentTimeMillis() / 1000
            val text = if (fullAt > nowS) {
                context.getString(R.string.resin_alert_full_at, hhmm(fullAt * 1000))
            } else {
                context.getString(R.string.resin_alert_full)
            }
            val notification = Notification.Builder(context, ALERTS)
                .setSmallIcon(R.drawable.ic_droplet)
                .setContentTitle(context.getString(R.string.resin_alert_title, target, Today.RESIN_MAX))
                .setContentText(text)
                .setContentIntent(open)
                .setAutoCancel(true)
                .build()
            context.getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification)
        }

        internal fun fired(context: Context, intent: Intent) {
            val store = context.getSharedPreferences(STORE, Context.MODE_PRIVATE)
            store.edit().putString(SENT, store.getString(PENDING, null)).apply()
            show(context, intent.getIntExtra(EXTRA_TARGET, 0), intent.getLongExtra(EXTRA_FULL_AT, 0L))
        }
    }
}

/** The resin alert's alarm. */
class ResinAlarm : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) = ResinAlert.fired(context, intent)
}
