// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

/**
 * The native wish sync's way to the tracker (`wish_sync.rs`): its requests go
 * through the app's sign-in, which holds and refreshes the tokens. Called on
 * the sync's own thread, so it may block.
 */
object WishTrackerBridge {
    @Volatile
    var tracker: Tracker? = null

    /** `"<status>\n<body>"`: 0 = unreachable (the body says why), -1 = signed out. */
    @JvmStatic
    fun call(method: String, path: String, body: String): String =
        runCatching { tracker?.wishCall(method, path, body) ?: "-1\n" }
            .getOrElse { "0\n" + (it.message ?: it.javaClass.simpleName) }
}
