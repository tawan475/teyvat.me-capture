// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdaterTest {
    @Test
    fun `a download is named by its version, anything else by nothing`() {
        assertEquals("1.2.0", Updater.versionOf(File("teyvatme-1.2.0.apk")))
        assertEquals("1.3.0-beta.1", Updater.versionOf(File("teyvatme-1.3.0-beta.1.apk")))
        assertNull(Updater.versionOf(File("teyvatme-1.2.0.apk.part")))
        assertNull(Updater.versionOf(File("notes.txt")))
    }

    @Test
    fun `a downloaded newer version is kept, the running one and older are not`() {
        // What the startup cleanup keeps: only a version newer than the one running.
        fun kept(name: String, running: String) =
            Updater.versionOf(File(name))?.let { Updater.newer(it, running) } ?: false
        assertTrue(kept("teyvatme-1.2.0.apk", "1.1.5"))
        assertFalse(kept("teyvatme-1.1.5.apk", "1.1.5"))
        assertFalse(kept("teyvatme-1.1.4.apk", "1.1.5"))
        assertFalse(kept("teyvatme-1.2.0.apk.part", "1.1.5"))
    }
}
