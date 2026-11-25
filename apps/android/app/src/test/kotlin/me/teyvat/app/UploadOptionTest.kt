// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UploadOptionTest {
    /** A tracker answer as plain maps (org.json is a stub in JVM unit tests). */
    private fun says(answer: Map<String, Any?>, option: String): Boolean =
        uploadOption(
            answer,
            option,
            { at, key -> @Suppress("UNCHECKED_CAST") (at[key] as? Map<String, Any?>) },
            { at, key -> at[key] },
        )

    @Test
    fun aFinalBatchIsTakenOnlyFromATrackerThatSaysSo() {
        val top = mapOf("upload" to mapOf("gzip" to true, "finalBatch" to true))
        assertTrue(says(top, "finalBatch"))
        assertTrue(says(top, "gzip"))
        // The old backend's wrapper.
        assertTrue(says(mapOf("status" to 200, "data" to mapOf("upload" to mapOf("finalBatch" to true))), "finalBatch"))
        // Absent, false, not a Boolean, not an object.
        assertFalse(says(mapOf("upload" to mapOf("gzip" to true)), "finalBatch"))
        assertFalse(says(mapOf("upload" to mapOf("finalBatch" to false)), "finalBatch"))
        assertFalse(says(mapOf("upload" to mapOf("finalBatch" to "true")), "finalBatch"))
        assertFalse(says(mapOf("upload" to true), "finalBatch"))
        // A body that is not JSON reads as an empty answer (`Tracker.json`).
        assertFalse(says(emptyMap(), "finalBatch"))
    }
}
