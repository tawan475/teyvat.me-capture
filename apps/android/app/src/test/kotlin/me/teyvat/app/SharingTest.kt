// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SharingTest {
    @Test
    fun `the names and their order are the contract's`() {
        // Must equal teyvatme_core::sharing::Part::ALL's names (the tracker reads them from gi_off).
        assertEquals(
            listOf(
                "characters", "weapons", "artifacts", "materials", "gi_player.profile", "gi_player.numbers",
                "gi_characters", "gi_achievements", "gi_anecdotes", "gi_tasks", "gi_endgame", "gi_exploration",
                "gi_collections", "gi_wardrobe", "gi_teapot", "gi_events", "gi_miliastra",
            ),
            SharePart.entries.map { it.key },
        )
        // The groups hold every part once, in that order.
        assertEquals(SharePart.entries.toList(), SharePart.GROUPS.flatMap { it.second })
    }

    @Test
    fun `a set of switches reads as its preset`() {
        assertEquals(SharePreset.Everything, SharePart.preset(emptySet(), false))
        assertEquals(SharePreset.Builds, SharePart.preset(SharePart.BUILDS_ONLY_OFF, false))
        assertEquals(SharePreset.Custom, SharePart.preset(setOf("gi_teapot"), false))
        // Custom stays picked while its set equals a preset's; an unknown name changes nothing.
        assertEquals(SharePreset.Custom, SharePart.preset(emptySet(), true))
        assertEquals(SharePreset.Everything, SharePart.preset(setOf("gi_future"), false))
        // Builds only keeps the four build sections and the account numbers.
        assertEquals(12, SharePart.BUILDS_ONLY_OFF.size)
        assertFalse("gi_player.numbers" in SharePart.BUILDS_ONLY_OFF)
        assertTrue("gi_player.profile" in SharePart.BUILDS_ONLY_OFF)
    }

    @Test
    fun `the native side gets the names in the contract's order`() {
        assertEquals("", SharePart.names(emptySet()))
        assertEquals("weapons,gi_teapot", SharePart.names(setOf("gi_teapot", "gi_future", "weapons")))
    }

    @Test
    fun `the UID goes while uploads are placed by it`() {
        assertTrue(SharePart.routesByUid("uid", signedIn = true, key = null))
        assertFalse(SharePart.routesByUid("account", signedIn = true, key = null))
        // A user key places by UID, an account key does not; nothing to upload with counts as by UID.
        assertTrue(SharePart.routesByUid(null, signedIn = false, key = "gdt_uk_abc"))
        assertFalse(SharePart.routesByUid(null, signedIn = false, key = "gdt_import_12_abc"))
        assertTrue(SharePart.routesByUid(null, signedIn = false, key = null))
    }
}
