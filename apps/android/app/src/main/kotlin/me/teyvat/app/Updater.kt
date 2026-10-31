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
     * Downloaded and checked; it goes in only when the user taps Install now
     * (an install started in the background had Play Protect's scan pop up
     * over whatever the user was doing).
     */
    data class Ready(val version: String) : UpdateState()
    /** A session is being written, or Android is installing it after its Update screen's tap. */
    data class Installing(val version: String) : UpdateState()
    /**
     * Android's Update screen is waiting for its tap (or was left open):
     * Install now shows that screen again, for the same session.
     */
    data class Confirming(val version: String) : UpdateState()
    /** Android needs "Install unknown apps" allowed for this app first. */
    data class NeedsPermission(val version: String) : UpdateState()
    data class Failed(val version: String?, val error: String) : UpdateState()
    data object UpToDate : UpdateState()
}

/**
 * The desktop's updater for the phone: the newest GitHub release's
 * `teyvatme-android.apk`, checked against its `SHA256SUMS.txt`, installed
 * with Android's PackageInstaller. Checking and downloading run on their
 * own (when updating automatically is on); installing only ever starts from
 * Install now, with the app open. On Android 12+ an update this app installs
 * needs no further tap once the app installed itself before; otherwise
 * Android shows its one-tap Update screen.
 */
class Updater(
    private val context: Context,
    private val prefs: Prefs,
    private val scope: CoroutineScope,
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

    /** Check now (About's button, or the timer). Not while an install is under way: it would replace its state. */
    fun check(manual: Boolean = true) {
        if (job?.isActive == true) return
        if (stateFlow.value is UpdateState.Installing || stateFlow.value is UpdateState.Confirming) return
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
        stateFlow.value = UpdateState.Ready(release.version)
        NativeBridge.note("info", context.getString(R.string.update_ready, release.version))
    }

    /**
     * The session this app committed last; answers about any other (one
     * abandoned for it, or one from before a restart) are stale. -1: none.
     */
    @Volatile
    private var session = -1

    /** Android's Update screen for [session], from its PENDING_USER_ACTION answer. */
    private var confirmIntent: Intent? = null

    private fun sessionAlive(id: Int): Boolean =
        id != -1 && runCatching { context.packageManager.packageInstaller.getSessionInfo(id) != null }.getOrDefault(false)

    /**
     * The app is back on screen. An install whose session Android ended
     * without an answer (gone from its list) is ready again; one still
     * there is left alone, since Android may be installing it right now.
     */
    fun onResume() {
        val version = when (val s = stateFlow.value) {
            is UpdateState.Installing -> s.version
            is UpdateState.Confirming -> s.version
            else -> return
        }
        if (session == -1 && stateFlow.value is UpdateState.Installing) return // still being written
        if (!sessionAlive(session)) {
            session = -1
            confirmIntent = null
            stateFlow.value = UpdateState.Ready(version)
        }
    }

    /** Install now (the card's button), even while capturing. */
    fun installNow() {
        when (val s = stateFlow.value) {
            is UpdateState.Ready -> install(s.version)
            is UpdateState.NeedsPermission -> install(s.version)
            // The same session's screen again: a new session would abandon
            // this one, and its answer (aborted) used to undo the new one.
            is UpdateState.Confirming -> {
                val screen = confirmIntent
                if (screen != null && sessionAlive(session)) confirm(context, Intent(screen), s.version) else install(s.version)
            }
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
        session = -1
        confirmIntent = null
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
                session = id
                installer.openSession(id).use { session ->
                    session.openWrite("teyvatme.apk", 0, file.length()).use { out ->
                        file.inputStream().use { it.copyTo(out) }
                        session.fsync(out)
                    }
                    val status = PendingIntent.getBroadcast(
                        context, id,
                        Intent(context, InstallResult::class.java).putExtra(EXTRA_VERSION, version).putExtra(EXTRA_SESSION, id),
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE,
                    )
                    session.commit(status.intentSender)
                }
            }.onFailure { e ->
                Log.w(TAG, "update install failed: $e")
                session = -1
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
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
        val id = intent.getIntExtra(EXTRA_SESSION, -1)
        if (id != session) {
            Log.i(TAG, "update: status $status for session $id ignored (current $session)")
            return
        }
        when (status) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                // Android's own Update screen: one tap.
                @Suppress("DEPRECATION")
                val confirm = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
                } else {
                    intent.getParcelableExtra(Intent.EXTRA_INTENT)
                }
                if (confirm != null && version != null) {
                    confirmIntent = Intent(confirm)
                    stateFlow.value = UpdateState.Confirming(version)
                    confirm(context, confirm, version)
                }
            }
            PackageInstaller.STATUS_SUCCESS -> {} // This process ends as the new version installs.
            else -> {
                session = -1
                confirmIntent = null
                val raw = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: "status $status"
                // Play Protect ("Harmful app blocked") or another Android
                // check refused it: say so, not the raw status, so a retry
                // that is refused again doesn't read as the updater looping.
                val message = if (status == PackageInstaller.STATUS_FAILURE_BLOCKED) {
                    context.getString(R.string.update_blocked)
                } else {
                    raw
                }
                // Cancelled on Android's screen: keep it ready for Install now.
                stateFlow.value = if (status == PackageInstaller.STATUS_FAILURE_ABORTED && version != null) {
                    UpdateState.Ready(version)
                } else {
                    UpdateState.Failed(version, message)
                }
                if (status != PackageInstaller.STATUS_FAILURE_ABORTED) {
                    NativeBridge.note("error", context.getString(R.string.update_failed, if (message == raw) raw else "$message: $raw"))
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
        private const val EXTRA_SESSION = "session"

        /** MainActivity is on screen (resumed). */
        @Volatile
        var foreground = false

        /** Not the resin alert's 20: each replaced the other. */
        private const val NOTIFICATION_UPDATE = 21

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
