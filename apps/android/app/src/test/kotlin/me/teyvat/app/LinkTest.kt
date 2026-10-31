// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import org.junit.Assert.assertEquals
import org.junit.Test

class LinkTest {
    @Test
    fun theTrackerHearsStoppedRunningOrDecoding() {
        fun state(vpn: VpnState, phase: String) = presenceState(vpn, Summary(phase = phase))
        assertEquals("stopped", state(VpnState.Off, "decrypting"))
        assertEquals("stopped", state(VpnState.On, "waiting"))
        assertEquals("stopped", state(VpnState.Starting, "waiting"))
        assertEquals("decoding", state(VpnState.On, "decrypting"))
        assertEquals("decoding", state(VpnState.On, "done"))
        for (phase in listOf("gameDetected", "loginSeen", "launchMissed", "keyLost")) {
            assertEquals(phase, "running", state(VpnState.On, phase))
        }
    }

    @Test
    fun resinTargetsAreOrderedAndNotRepeated() {
        assertEquals(listOf(160, 200), TrackerAlerts(160, true).targets())
        assertEquals(listOf(200), TrackerAlerts(200, true).targets())
        assertEquals(listOf(200), TrackerAlerts(900, false).targets())
        assertEquals(listOf(200), TrackerAlerts(null, true).targets())
        assertEquals(emptyList<Int>(), TrackerAlerts(null, false).targets())
    }
}
