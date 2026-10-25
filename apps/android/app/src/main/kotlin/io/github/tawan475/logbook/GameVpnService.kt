// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package io.github.tawan475.logbook

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.drawable.Icon
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.VpnService
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import java.io.File
import java.net.Inet6Address
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * A VPN for the game alone: only [GAME_PACKAGE] is routed into the tunnel,
 * and the native forwarder sends its traffic on unchanged while copying the
 * game's KCP packets into a pcap file in the app's files folder.
 *
 * Runs as a foreground service (type `systemExempted`, which Android 14+
 * allows for an app holding the VPN consent) with a notification that can
 * stop it, so it keeps running when the activity is closed.
 *
 * IPv6 is routed only when the underlying network has it at start
 * ([hasGlobalIpv6]). Without a route Android blocks the family for the game
 * (no `allowFamily`), so the game sees an IPv4-only network and never tries
 * IPv6 at all — better than offering IPv6 the phone can't carry. The tunnel's
 * own IPv6 address is a ULA, so for hosts that have both families the game's
 * address selection (RFC 6724) still prefers IPv4, as most of its traffic did
 * before; IPv6-only hosts work. If a later network switch loses IPv6, the
 * forwarder's TCP gate refuses each IPv6 connect before the handshake, so the
 * game's Happy Eyeballs falls back to IPv4 at once.
 *
 * The game's DNS server is a virtual address the forwarder sends on to the
 * current network's resolvers, so a Wi-Fi ↔ mobile switch only needs
 * [onDefaultNetwork] (underlying network + resolvers), not a new tunnel.
 */
class GameVpnService : VpnService() {
    private var connectivity: ConnectivityManager? = null
    private var currentNetwork: Network? = null
    private var currentAddresses: List<String> = emptyList()

    private val networkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            connectivity?.getLinkProperties(network)?.let { onDefaultNetwork(network, it) }
        }

        override fun onLinkPropertiesChanged(network: Network, lp: LinkProperties) = onDefaultNetwork(network, lp)

        override fun onLost(network: Network) {
            Log.i(TAG, "default network lost: $network")
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        Log.i(TAG, "onStartCommand action=${intent?.action}")
        when (intent?.action) {
            ACTION_STOP -> stopVpn()
            else -> startVpn()
        }
        return START_STICKY
    }

    private fun startVpn() {
        // Foreground first: a service started with startForegroundService
        // must get there even when establishing fails below.
        if (!goForeground()) {
            stopSelf()
            return
        }
        if (NativeBridge.stats().isNotEmpty()) return
        val connectivity = getSystemService(ConnectivityManager::class.java)
        this.connectivity = connectivity
        val network = connectivity.activeNetwork
        val lp = network?.let { connectivity.getLinkProperties(it) }
        val ipv6 = lp != null && hasGlobalIpv6(lp)
        val builder = Builder()
            .setSession("Logbook")
            .setMtu(1500)
            .setMetered(false) // follow the underlying network instead
            .addAddress("10.111.0.2", 32)
            .addRoute("0.0.0.0", 0)
            .addDnsServer(VIRTUAL_DNS)
            .addAllowedApplication(GAME_PACKAGE)
        if (ipv6) builder.addAddress(TUN_IPV6, 128).addRoute("::", 0)
        if (network != null) builder.setUnderlyingNetworks(arrayOf(network))

        val tun = builder.establish() ?: run {
            Log.e(TAG, "establish() returned null (VPN not prepared?)")
            stopVpn()
            return
        }
        val stamp = SimpleDateFormat("yyyy-MM-dd_HH-mm-ss", Locale.ROOT).format(Date())
        val pcap = File(filesDir, "game-$stamp.pcap")
        val fd = tun.detachFd()
        val resolvers = lp?.let(::resolverList).orEmpty()
        if (NativeBridge.start(fd, pcap.path, resolvers) != 0) {
            Log.e(TAG, "native start failed")
            stopVpn()
            return
        }
        currentNetwork = network
        currentAddresses = lp?.let(::addressList).orEmpty()
        watchUnderlyingNetwork(connectivity)
        lastPcap = pcap.path
        Log.i(TAG, "VPN up for $GAME_PACKAGE, ipv6=$ipv6, resolvers=$resolvers, pcap=${pcap.path}")
    }

    /**
     * Follows the network the phone would use without this VPN. The default
     * network callback reports the VPN itself once it is up (seen on Android
     * 16), so on 12+ ask for the best network that is not a VPN; older
     * versions keep the default callback and skip VPNs in [onDefaultNetwork].
     */
    private fun watchUnderlyingNetwork(connectivity: ConnectivityManager) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val request = NetworkRequest.Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
                .build()
            connectivity.registerBestMatchingNetworkCallback(request, networkCallback, Handler(Looper.getMainLooper()))
        } else {
            connectivity.registerDefaultNetworkCallback(networkCallback)
        }
    }

    /**
     * The phone's underlying network appeared or changed. A different network, or new
     * addresses on the same one, makes the forwarder's UDP flows reconnect
     * from the new address; resolvers are refreshed either way.
     */
    private fun onDefaultNetwork(network: Network, lp: LinkProperties) {
        val vpn = connectivity?.getNetworkCapabilities(network)?.hasTransport(NetworkCapabilities.TRANSPORT_VPN) ?: false
        if (vpn) return
        val addresses = addressList(lp)
        val moved = network != currentNetwork || addresses != currentAddresses
        currentNetwork = network
        currentAddresses = addresses
        setUnderlyingNetworks(arrayOf(network))
        val resolvers = resolverList(lp)
        NativeBridge.networkChanged(resolvers, moved)
        if (moved) Log.i(TAG, "default network now $network (${lp.interfaceName}), ipv6=${hasGlobalIpv6(lp)}, resolvers=$resolvers")
    }

    /**
     * False when Android refuses: `systemExempted` needs the VPN consent,
     * which a sticky restart may find revoked.
     */
    private fun goForeground(): Boolean {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Capture", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Shown while Logbook captures the game"
                setShowBadge(false)
            },
        )
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE,
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, GameVpnService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_capture)
            .setContentTitle("Capturing Genshin Impact")
            .setContentText("Only the game's traffic goes through Logbook")
            .setContentIntent(open)
            .setOngoing(true)
            .setCategory(Notification.CATEGORY_SERVICE)
            .addAction(Notification.Action.Builder(null as Icon?, "Stop", stop).build())
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            notification.setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
        }
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                startForeground(NOTIFICATION_ID, notification.build(), ServiceInfo.FOREGROUND_SERVICE_TYPE_SYSTEM_EXEMPTED)
            } else {
                startForeground(NOTIFICATION_ID, notification.build())
            }
            true
        } catch (e: RuntimeException) {
            Log.e(TAG, "startForeground refused", e)
            false
        }
    }

    private fun stopVpn() {
        connectivity?.let { runCatching { it.unregisterNetworkCallback(networkCallback) } }
        connectivity = null
        NativeBridge.stop()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onRevoke() {
        Log.i(TAG, "VPN revoked")
        stopVpn()
        super.onRevoke()
    }

    override fun onDestroy() {
        connectivity?.let { runCatching { it.unregisterNetworkCallback(networkCallback) } }
        NativeBridge.stop()
        super.onDestroy()
    }

    companion object {
        const val GAME_PACKAGE = "com.miHoYo.GenshinImpact"
        const val ACTION_STOP = "io.github.tawan475.logbook.STOP"
        /** The forwarder's virtual resolver (`VIRTUAL_DNS` in android/native). */
        private const val VIRTUAL_DNS = "10.111.0.53"
        private const val TUN_IPV6 = "fd00:6972:6d::2"
        private const val CHANNEL = "capture"
        private const val NOTIFICATION_ID = 1
        private const val TAG = "logbook"

        @Volatile
        var lastPcap: String? = null

        /** A global (not link-local, not ULA) IPv6 address and an IPv6 default route. */
        fun hasGlobalIpv6(lp: LinkProperties): Boolean {
            val address = lp.linkAddresses.any {
                val a = it.address
                a is Inet6Address && !a.isLinkLocalAddress && !a.isLoopbackAddress &&
                    (a.address[0].toInt() and 0xfe) != 0xfc
            }
            val route = lp.routes.any { it.isDefaultRoute && it.destination.address is Inet6Address }
            return address && route
        }

        private fun resolverList(lp: LinkProperties) = lp.dnsServers.joinToString(",") { it.hostAddress.orEmpty() }

        private fun addressList(lp: LinkProperties) = lp.linkAddresses.map { it.toString() }.sorted()
    }
}
