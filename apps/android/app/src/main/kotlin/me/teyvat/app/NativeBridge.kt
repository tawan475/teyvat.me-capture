// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

/** The Rust forwarder in `android/native` (libteyvatme_android.so). */
object NativeBridge {
    init {
        System.loadLibrary("teyvatme_android")
    }

    /**
     * Takes ownership of [fd] (a detached tun descriptor). [resolvers] is the
     * underlying network's DNS servers, comma-separated. 0 on success.
     */
    external fun start(fd: Int, pcapPath: String, resolvers: String): Int

    /**
     * The underlying network's resolvers now; [newNetwork] when the default
     * network (or its addresses) changed, so UDP flows reconnect from it.
     */
    external fun networkChanged(resolvers: String, newNetwork: Boolean)

    external fun stop()

    /** One line of counters, or "" when not running. */
    external fun stats(): String
}
