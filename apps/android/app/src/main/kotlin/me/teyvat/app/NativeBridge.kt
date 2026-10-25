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
}
