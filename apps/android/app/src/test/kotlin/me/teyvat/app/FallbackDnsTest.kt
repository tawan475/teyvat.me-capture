// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import java.net.InetAddress
import java.net.UnknownHostException
import okhttp3.Dns
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class FallbackDnsTest {
    private val real = listOf(InetAddress.getByName("192.0.2.10"))
    private val sinkhole = listOf(InetAddress.getByName("0.0.0.0"), InetAddress.getByName("::"))

    private class Counted(val answer: () -> List<InetAddress>) : Dns {
        var calls = 0
        override fun lookup(hostname: String): List<InetAddress> {
            calls++
            return answer()
        }
    }

    @Test
    fun `the system's answer is used when it is usable`() {
        val secure = Counted { error("not asked") }
        assertEquals(real, FallbackDns(Counted { real }, listOf(secure)).lookup("teyvat.me"))
        assertEquals(0, secure.calls)
    }

    @Test
    fun `a 0_0_0_0 answer goes to DNS over HTTPS`() {
        assertEquals(real, FallbackDns(Counted { sinkhole }, listOf(Counted { real })).lookup("teyvat.me"))
    }

    @Test
    fun `a failed system lookup goes to DNS over HTTPS, in order`() {
        val first = Counted { throw UnknownHostException("down") }
        val second = Counted { real }
        val third = Counted { error("not asked") }
        val dns = FallbackDns(Counted { throw UnknownHostException("no answer") }, listOf(first, second, third))
        assertEquals(real, dns.lookup("teyvat.me"))
        assertEquals(1, first.calls)
        assertEquals(0, third.calls)
    }

    @Test
    fun `nothing usable anywhere says what the network answered`() {
        val e = assertThrows(UnknownHostException::class.java) {
            FallbackDns(Counted { sinkhole }, listOf(Counted { sinkhole })).lookup("teyvat.me")
        }
        assertTrue(e.message!!, e.message!!.contains("answered 0.0.0.0"))
    }

    @Test
    fun `loopback stays usable for a local tracker`() {
        val loopback = listOf(InetAddress.getByName("127.0.0.1"))
        assertEquals(loopback, FallbackDns(Counted { loopback }, emptyList()).lookup("localhost"))
    }
}
