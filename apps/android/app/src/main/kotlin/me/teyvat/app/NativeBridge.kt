// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

/** The Rust side in `apps/android/native` (libteyvatme_android.so). */
object NativeBridge {
    init {
        System.loadLibrary("teyvatme_android")
    }

    /**
     * Logging to logcat and [logDir]/latest.log, and the decoder, which loads
     * the game data in the background. Once per process.
     */
    external fun init(logDir: String)

    /**
     * Takes ownership of [fd] (a detached tun descriptor). [resolvers] is the
     * underlying network's DNS servers, comma-separated; [pcapPath] is "" unless
     * the capture is recorded for a bug report. 0 on success.
     */
    external fun start(fd: Int, pcapPath: String, resolvers: String): Int

    /**
     * The underlying network's resolvers now; [newNetwork] when the default
     * network (or its addresses) changed, so UDP flows reconnect from it.
     */
    external fun networkChanged(resolvers: String, newNetwork: Boolean)

    external fun stop()

    /** One line of the forwarder's counters, or "" when not running. */
    external fun stats(): String

    /** The decoder's state as JSON ([Summary.parse]). */
    external fun summary(): String

    /**
     * The GOOD JSON with [settings] ([ExportFilters.toJson]; "" for the
     * defaults), or null when nothing is captured yet.
     */
    external fun exportGood(settings: String): String?

    /** `{"gi_achievements": [...]}`, or null before achievements were captured. */
    external fun achievements(): String?

    /** An event for the activity list; [kind] is `ok`, `error` or `info`. */
    external fun note(kind: String, text: String)

    /**
     * The game's wish history link for this login's key, or null. A password
     * while it lasts: never log it.
     */
    external fun wishUrl(): String?

    /**
     * Start reading every banner into a UIGF file: 0 started, 1 no key, 2 the
     * key's UID is not known yet, 3 a read is running.
     */
    external fun startWishHistory(appVersion: String): Int

    external fun cancelWishHistory()

    /** The last read's UIGF v4 JSON, or null. */
    external fun wishHistory(): String?

    /**
     * Whether a captured wish key is synced to the tracker (signed in, uploads
     * and wishes on). The sync's requests come back through [WishTrackerBridge].
     */
    external fun setWishSync(on: Boolean)

    /**
     * The tracker's snapshot (the whole account, every filter off, with live
     * sync's labels) as `"<ticket>\n<GOOD JSON>"`; "" when nothing is due
     * ([live] mode: batches carry the changes); null before data is captured.
     */
    external fun trackerExport(manual: Boolean, live: Boolean): String?

    /** When live sync next has something to do (Unix ms), or -1. */
    external fun liveNextTick(live: Boolean): Long

    /** `"S<ticket>\n<GOOD JSON>"` (a snapshot), `"B\n<delta JSON>"` (a batch), or null. */
    external fun liveTick(live: Boolean): String?

    /** A snapshot upload finished: the tracker's answer, or null when it failed. */
    external fun snapshotAnswered(ticket: Long, body: String?)

    /** A batch finished: its HTTP status (0: no answer, -1: signed out) and body. */
    external fun batchAnswered(status: Int, body: String)
}
