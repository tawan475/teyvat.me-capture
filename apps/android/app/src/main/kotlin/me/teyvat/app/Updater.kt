// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.os.Build
import android.provider.Settings as AndroidSettings
import android.util.Log
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** Where the self-update is (the Home card and About show it). */
sealed class UpdateState {
    data object Idle : UpdateState()
    data object Checking : UpdateState()
    data class Downloading(val version: String, val percent: Int) : UpdateState()
    /**
     * Downloaded and checked. `silent`: Android installs it without asking
     * (this app installed the running version), so it goes in once capture is
     * off and the app is off screen; otherwise only on Install now.
     */
    data class Ready(val version: String, val silent: Boolean = false) : UpdateState()
    data class Installing(val version: String) : UpdateState()
    /** Android needs "Install unknown apps" allowed for this app first. */
    data class NeedsPermission(val version: String) : UpdateState()
    data class Failed(val version: String?, val error: String) : UpdateState()
    data object UpToDate : UpdateState()
}

/**
 * The desktop's updater for the phone: the newest GitHub release's
 * `teyvatme-android.apk`, checked against its `SHA256SUMS.txt`, installed
 * with Android's PackageInstaller. On Android 12+ an update this app
 * installs needs no tap once the app installed itself before; otherwise
 * Android shows its one-tap Update screen. A running capture is never cut:
 * a ready update waits until capture stops.
 */
class Updater(
    private val context: Context,
    private val prefs: Prefs,
    private val scope: CoroutineScope,
    private val capturing: () -> Boolean,
) {
    private val stateFlow = MutableStateFlow<UpdateState>(UpdateState.Idle)
    val state: StateFlow<UpdateState> = stateFlow.asStateFlow()
    private var job: Job? = null
    private var lastCheck = 0L
    private val dir get() = File(context.cacheDir, "update").apply { mkdirs() }

    init {
        instance = this
        // Leftovers of an update that installed, or of an older version. A
        // newer one stays: it was downloaded and checked, and is still to go in.
        dir.listFiles()
            ?.filter { file -> versionOf(file)?.let { !newer(it, BuildConfig.VERSION_NAME) } ?: true }
            ?.forEach { it.delete() }
        scope.launch {
            delay(5_000)
            while (true) {
                if (prefs.value.autoUpdate) check(manual = false)
                delay(CHECK_EVERY_MS)
            }
        }
    }

    /** Check now (About's button, or the timer). */
    fun check(manual: Boolean = true) {
        if (job?.isActive == true) return
        if (!manual && System.currentTimeMillis() - lastCheck < CHECK_EVERY_MS / 2) return
        lastCheck = System.currentTimeMillis()
        job = scope.launch {
            if (manual) stateFlow.value = UpdateState.Checking
            val result = withContext(Dispatchers.IO) { runCatching { latest() } }
            val release = result.getOrElse { e ->
                Log.w(TAG, "update check failed: $e")
                stateFlow.value = if (manual) UpdateState.Failed(null, e.message ?: e.javaClass.simpleName) else UpdateState.Idle
                return@launch
            }
            if (release == null || !newer(release.version, BuildConfig.VERSION_NAME)) {
                stateFlow.value = if (manual) UpdateState.UpToDate else UpdateState.Idle
                return@launch
            }
            download(release)
        }
    }

    private class Release(val version: String, val apk: String, val sums: String)

    private fun latest(): Release? {
        val o = JSONObject(get(LATEST_URL).toString(Charsets.UTF_8))
        val version = o.getString("tag_name").removePrefix("v")
        val assets = o.getJSONArray("assets")
        var apk: String? = null
        var sums: String? = null
        for (i in 0 until assets.length()) {
            val a = assets.getJSONObject(i)
            when (a.getString("name")) {
                APK_ASSET -> apk = a.getString("browser_download_url")
                SUMS_ASSET -> sums = a.getString("browser_download_url")
            }
        }
        return if (apk != null && sums != null) Release(version, apk, sums) else null
    }

    private suspend fun download(release: Release) {
        val file = File(dir, apkName(release.version))
        val ok = withContext(Dispatchers.IO) {
            runCatching {
                val sums = get(release.sums).toString(Charsets.UTF_8)
                val want = sums.lineSequence()
                    .map { it.trim().split(Regex("\\s+"), limit = 2) }
                    .firstOrNull { it.size == 2 && it[1].trimStart('*') == APK_ASSET }
                    ?.get(0)?.lowercase() ?: throw IOException("$APK_ASSET is not in $SUMS_ASSET")
                if (!file.exists() || sha256(file) != want) {
                    stateFlow.value = UpdateState.Downloading(release.version, 0)
                    val part = File(dir, file.name + ".part")
                    fetch(release.apk, part) { percent -> stateFlow.value = UpdateState.Downloading(release.version, percent) }
                    if (sha256(part) != want) {
                        part.delete()
                        throw IOException("The download does not match its checksum")
                    }
                    part.renameTo(file)
                }
            }
        }
        ok.onFailure { e ->
            Log.w(TAG, "update download failed: $e")
            stateFlow.value = UpdateState.Failed(release.version, e.message ?: e.javaClass.simpleName)
            NativeBridge.note("error", context.getString(R.string.update_failed, e.message ?: ""))
            return
        }
        stateFlow.value = UpdateState.Ready(release.version, installsSilently())
        NativeBridge.note("info", context.getString(R.string.update_ready, release.version))
        installWhenFree()
    }

    /**
     * Whether Android installs an update from this app without asking: from
     * Android 12, when this app installed the version that runs (after its
     * first self-update). A copy installed from the browser or a file manager
     * needs a tap every time, and a background app may not show that screen.
     */
    private fun installsSilently(): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return false
        return runCatching {
            context.packageManager.getInstallSourceInfo(context.packageName).installingPackageName == context.packageName
        }.getOrDefault(false)
    }

    /**
     * Installs a ready update when nothing is cut by it: capture off and the
     * app not on screen (installing ends this process). Otherwise the card's
     * Install now does it, and [onCaptureStopped] / the app leaving the
     * screen try again. An update Android would ask about can't be asked from
     * the background, so then a notification says it is ready instead.
     */
    fun installWhenFree() {
        val ready = stateFlow.value as? UpdateState.Ready ?: return
        if (!prefs.value.autoUpdate || capturing() || foreground) return
        if (ready.silent) install(ready.version) else notifyReady(ready.version)
    }

    private var notifiedVersion: String? = null

    /** "Update ready, tap to install": opens the app, which installs it. Once per version. */
    private fun notifyReady(version: String) {
        if (notifiedVersion == version) return
        notifiedVersion = version
        GameVpnService.createChannels(context)
        val open = Intent(context, MainActivity::class.java)
            .setAction(MainActivity.ACTION_INSTALL_UPDATE)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        val tap = PendingIntent.getActivity(
            context, NOTIFICATION_UPDATE, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val note = android.app.Notification.Builder(context, "alerts")
            .setSmallIcon(R.drawable.ic_capture)
            .setContentTitle(context.getString(R.string.update_ready, version))
            .setContentText(context.getString(R.string.update_tap_to_install))
            .setContentIntent(tap)
            .setAutoCancel(true)
            .build()
        runCatching { context.getSystemService(android.app.NotificationManager::class.java).notify(NOTIFICATION_UPDATE, note) }
    }

    private var installStartedAt = 0L

    /**
     * The app is back on screen. An install still "in progress" whose screen
     * was left with Home (Android then never answers) is ready again, for
     * Install now to open a fresh one.
     */
    fun onResume() {
        val installing = stateFlow.value as? UpdateState.Installing ?: return
        if (System.currentTimeMillis() - installStartedAt > RESUME_GRACE_MS) {
            stateFlow.value = UpdateState.Ready(installing.version, installsSilently())
        }
    }

    /** Capture just stopped: a waiting update goes in now. */
    fun onCaptureStopped() = installWhenFree()

    /** Install now (the card's button), even while capturing. */
    fun installNow() {
        when (val s = stateFlow.value) {
            is UpdateState.Ready -> install(s.version)
            is UpdateState.NeedsPermission -> install(s.version)
            else -> {}
        }
    }

    private fun install(version: String) {
        val file = File(dir, apkName(version))
        if (!file.exists()) {
            stateFlow.value = UpdateState.Idle
            return
        }
        if (!context.packageManager.canRequestPackageInstalls()) {
            stateFlow.value = UpdateState.NeedsPermission(version)
            return
        }
        stateFlow.value = UpdateState.Installing(version)
        installStartedAt = System.currentTimeMillis()
        scope.launch(Dispatchers.IO) {
            runCatching {
                val installer = context.packageManager.packageInstaller
                // One at a time: a screen left open earlier would otherwise
                // come back later, beside this one.
                installer.mySessions.forEach { runCatching { installer.abandonSession(it.sessionId) } }
                val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
                    setAppPackageName(context.packageName)
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                        setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
                    }
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                        setRequestUpdateOwnership(true)
                    }
                }
                val id = installer.createSession(params)
                installer.openSession(id).use { session ->
                    session.openWrite("teyvatme.apk", 0, file.length()).use { out ->
                        file.inputStream().use { it.copyTo(out) }
                        session.fsync(out)
                    }
                    val status = PendingIntent.getBroadcast(
                        context, id, Intent(context, InstallResult::class.java).putExtra(EXTRA_VERSION, version),
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE,
                    )
                    session.commit(status.intentSender)
                }
            }.onFailure { e ->
                Log.w(TAG, "update install failed: $e")
                stateFlow.value = UpdateState.Failed(version, e.message ?: e.javaClass.simpleName)
            }
        }
    }

    /** Settings › Install unknown apps, for this app. */
    fun permissionIntent(): Intent =
        Intent(AndroidSettings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, android.net.Uri.parse("package:${context.packageName}"))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    internal fun installResult(intent: Intent) {
        val version = intent.getStringExtra(EXTRA_VERSION)
        when (val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                // Android's own Update screen: one tap.
                @Suppress("DEPRECATION")
                val confirm = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
                } else {
                    intent.getParcelableExtra(Intent.EXTRA_INTENT)
                }
                if (confirm != null) confirm(context, confirm, version)
            }
            PackageInstaller.STATUS_SUCCESS -> {} // This process ends as the new version installs.
            else -> {
                val message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: "status $status"
                // Cancelled on Android's screen: keep it ready for Install now.
                stateFlow.value = if (status == PackageInstaller.STATUS_FAILURE_ABORTED && version != null) {
                    UpdateState.Ready(version, installsSilently())
                } else {
                    UpdateState.Failed(version, message)
                }
                if (status != PackageInstaller.STATUS_FAILURE_ABORTED) {
                    NativeBridge.note("error", context.getString(R.string.update_failed, message))
                }
            }
        }
    }

    private fun get(url: String): ByteArray {
        val c = open(url)
        try {
            ok(c)
            return c.inputStream.use { it.readBytes() }
        } finally {
            c.disconnect()
        }
    }

    private fun fetch(url: String, to: File, progress: (Int) -> Unit) {
        val c = open(url)
        try {
            ok(c)
            val total = c.contentLengthLong
            var read = 0L
            var shown = -1
            c.inputStream.use { input ->
                to.outputStream().use { out ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val n = input.read(buffer)
                        if (n < 0) break
                        out.write(buffer, 0, n)
                        read += n
                        if (total > 0) {
                            val percent = (read * 100 / total).toInt()
                            if (percent != shown) {
                                shown = percent
                                progress(percent)
                            }
                        }
                    }
                }
            }
        } finally {
            c.disconnect()
        }
    }

    private fun open(url: String): HttpURLConnection = (URL(url).openConnection() as HttpURLConnection).apply {
        connectTimeout = 15_000
        readTimeout = 60_000
        instanceFollowRedirects = true
        setRequestProperty("User-Agent", "Teyvatme-Android/${BuildConfig.VERSION_NAME}")
        setRequestProperty("Accept", "application/vnd.github+json, application/octet-stream")
    }

    private fun ok(c: HttpURLConnection) {
        if (c.responseCode !in 200..299) throw IOException("HTTP ${c.responseCode}")
    }

    companion object {
        private const val TAG = "teyvatme"
        private const val LATEST_URL = "https://api.github.com/repos/tawan475/teyvat.me-capture/releases/latest"
        private const val APK_ASSET = "teyvatme-android.apk"
        private const val SUMS_ASSET = "SHA256SUMS.txt"
        private const val CHECK_EVERY_MS = 6 * 60 * 60_000L
        private const val EXTRA_VERSION = "version"

        /** MainActivity is on screen (resumed). */
        @Volatile
        var foreground = false

        private const val NOTIFICATION_UPDATE = 20

        /**
         * Android's one-tap Update screen: opened when the app is on screen,
         * else a notification (Android doesn't let a background app open it).
         */
        internal fun confirm(context: Context, confirm: Intent, version: String?) {
            confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            if (foreground) {
                runCatching { context.startActivity(confirm) }.onSuccess { return }
            }
            GameVpnService.createChannels(context)
            val tap = PendingIntent.getActivity(
                context, NOTIFICATION_UPDATE, confirm, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            val note = android.app.Notification.Builder(context, "alerts")
                .setSmallIcon(R.drawable.ic_capture)
                .setContentTitle(context.getString(R.string.update_ready, version ?: ""))
                .setContentText(context.getString(R.string.update_tap_to_install))
                .setContentIntent(tap)
                .setAutoCancel(true)
                .build()
            runCatching { context.getSystemService(android.app.NotificationManager::class.java).notify(NOTIFICATION_UPDATE, note) }
        }

        /** The running app's updater, for the install result. */
        @Volatile
        var instance: Updater? = null
            private set

        private fun apkName(version: String) = "teyvatme-$version.apk"

        private val APK_NAME = Regex("teyvatme-([0-9][0-9A-Za-z.+-]*)\\.apk")

        /** The version a download's file name holds, or `null` for any other file (a `.part`). */
        internal fun versionOf(file: File): String? = APK_NAME.matchEntire(file.name)?.groupValues?.get(1)

        /** How long after Install now a return to the app still counts as Android's screen opening. */
        private const val RESUME_GRACE_MS = 3_000L

        private fun sha256(file: File): String {
            val digest = MessageDigest.getInstance("SHA-256")
            file.inputStream().use { input ->
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    val n = input.read(buffer)
                    if (n < 0) break
                    digest.update(buffer, 0, n)
                }
            }
            return digest.digest().joinToString("") { "%02x".format(it) }
        }

        /**
         * Semver precedence, as the desktop's `bump_is_greater`: numbers
         * first; a release beats its own pre-release.
         */
        fun newer(candidate: String, current: String): Boolean {
            fun parts(v: String): Pair<List<Int>, String?> {
                val core = v.substringBefore('+')
                val pre = core.substringAfter('-', "").ifEmpty { null }
                val nums = core.substringBefore('-').split('.').map { it.toIntOrNull() ?: 0 }
                return (nums + List(3) { 0 }).take(3) to pre
            }
            val (a, ap) = parts(candidate)
            val (b, bp) = parts(current)
            for (i in 0 until 3) if (a[i] != b[i]) return a[i] > b[i]
            return when {
                ap == null && bp != null -> true
                ap != null && bp == null -> false
                ap == null -> false
                else -> ap > bp!!
            }
        }
    }
}

/** PackageInstaller's answer about a session this app committed. */
class InstallResult : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Updater.instance?.installResult(intent)
            ?: run {
                // The app was not running: only Android's Update screen needs showing.
                if (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, -1) == PackageInstaller.STATUS_PENDING_USER_ACTION) {
                    @Suppress("DEPRECATION")
                    (intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT))?.let {
                        Updater.confirm(context, it, intent.getStringExtra("version"))
                    }
                }
            }
    }
}
