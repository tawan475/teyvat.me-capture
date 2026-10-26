// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.util.Log
import java.net.InetAddress
import java.net.UnknownHostException
import java.util.concurrent.TimeUnit
import okhttp3.Dns
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.dnsoverhttps.DnsOverHttps

/**
 * The HTTP client for the tracker.
 *
 * Some networks' resolvers answer a new domain with 0.0.0.0 (a filter's
 * "blocked") or not at all, while the browser, on its own secure DNS, opens
 * the site fine. [FallbackDns] asks the system first and then Cloudflare's
 * and Google's DNS over HTTPS.
 */
object Net {
    val client: OkHttpClient by lazy {
        OkHttpClient.Builder()
            .dns(FallbackDns(Dns.SYSTEM, secure))
            .connectTimeout(15, TimeUnit.SECONDS)
            .readTimeout(60, TimeUnit.SECONDS)
            .build()
    }

    // Bootstrapped by address, so they never ask the resolver that failed.
    private val secure: List<Dns> by lazy {
        val bare = OkHttpClient.Builder().connectTimeout(10, TimeUnit.SECONDS).readTimeout(10, TimeUnit.SECONDS).build()
        listOf(
            DnsOverHttps.Builder().client(bare)
                .url("https://cloudflare-dns.com/dns-query".toHttpUrl())
                .bootstrapDnsHosts(InetAddress.getByName("1.1.1.1"), InetAddress.getByName("1.0.0.1"))
                .build(),
            DnsOverHttps.Builder().client(bare)
                .url("https://dns.google/dns-query".toHttpUrl())
                .bootstrapDnsHosts(InetAddress.getByName("8.8.8.8"), InetAddress.getByName("8.8.4.4"))
                .build(),
        )
    }
}

/** [system] first; each of [secure] in turn when it gives no usable address. */
internal class FallbackDns(private val system: Dns, private val secure: List<Dns>) : Dns {
    override fun lookup(hostname: String): List<InetAddress> {
        val answer = runCatching { system.lookup(hostname) }
        val usable = answer.getOrNull().orEmpty().filter(::usable)
        if (usable.isNotEmpty()) return usable
        val why = answer.exceptionOrNull()?.let { "failed (${it.message})" }
            ?: "answered ${answer.getOrNull().orEmpty().joinToString { it.hostAddress.orEmpty() }.ifEmpty { "nothing" }}"
        for (dns in secure) {
            val found = runCatching { dns.lookup(hostname) }.getOrNull().orEmpty().filter(::usable)
            if (found.isNotEmpty()) {
                // Log is a stub in unit tests.
                runCatching { Log.i("TeyvatmeNet", "$hostname: the network's DNS $why; using DNS over HTTPS") }
                return found
            }
        }
        throw UnknownHostException("$hostname: this network's DNS $why, and DNS over HTTPS failed too")
    }

    /** 0.0.0.0 and :: are what filters answer for a blocked name. */
    private fun usable(a: InetAddress) = !a.isAnyLocalAddress
}
