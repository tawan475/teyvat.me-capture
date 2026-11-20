// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import java.util.zip.GZIPInputStream
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class GzipTest {
    @Test
    fun aSnapshotComesBackWholeAndMuchSmaller() {
        val json = "{\"artifacts\":[" + "{\"setKey\":\"X\",\"name\":\"ฟ้า\"},".repeat(2_000) + "]}"
        val packed = gzipped(json)!!
        assertTrue(packed.size * 5 < json.toByteArray(Charsets.UTF_8).size)
        val back = GZIPInputStream(packed.inputStream()).use { it.readBytes().toString(Charsets.UTF_8) }
        assertEquals(json, back)
    }
}
