// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Home's Today clocks (spec 4.1.3), from synthetic `gi_tasks` values. */
class TodayTest {
    private val next = 1_800_000_000L

    @Test
    fun `resin counts up one every 8 minutes from nextAt and stops at the cap`() {
        val today = Today(resin = 142, resinNextAt = next)
        assertEquals(142, today.resinAt(next - 1))
        assertEquals(143, today.resinAt(next))
        assertEquals(143, today.resinAt(next + 479))
        assertEquals(144, today.resinAt(next + 480))
        assertEquals(200, today.resinAt(next + 1_000_000))
    }

    @Test
    fun `resin is full when the last of it comes`() {
        // 58 to go: the first at nextAt, then 57 more 8 minutes apart.
        val today = Today(resin = 142, resinNextAt = next)
        assertEquals(next + 57 * 480, today.resinFullAt())
        assertEquals(200, today.resinAt(next + 57 * 480))
        assertEquals(199, today.resinAt(next + 57 * 480 - 1))
    }

    @Test
    fun `no clock, no guess`() {
        val today = Today(resin = 142, resinNextAt = 0)
        assertEquals(142, today.resinAt(next + 10_000))
        assertNull(today.resinFullAt())
        assertNull(Today(resin = 200, resinNextAt = next).resinFullAt())
        assertNull(Today().resinAt(next))
    }

    @Test
    fun `expeditions come back in order, then all are back`() {
        val today = Today(expeditions = listOf(next + 300, next + 100, next + 200))
        assertEquals(next + 100, today.firstBack(next))
        assertEquals(next + 200, today.firstBack(next + 100))
        assertNull(today.firstBack(next + 300))
        assertNull(Today().firstBack(next))
    }

    @Test
    fun `the Abyss resets in whole days, rounded up`() {
        val today = Today(abyssEnd = next + 9 * 86_400)
        assertEquals(9, today.abyssDaysLeft(next))
        assertEquals(9, today.abyssDaysLeft(next + 1))
        assertEquals(1, today.abyssDaysLeft(next + 9 * 86_400 - 1))
        assertNull(today.abyssDaysLeft(next + 9 * 86_400))
    }
}
