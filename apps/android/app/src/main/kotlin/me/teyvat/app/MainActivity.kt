// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.provider.Settings as AndroidSettings
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.unit.dp
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import me.teyvat.app.ui.AboutScreen
import me.teyvat.app.ui.ActivityScreen
import me.teyvat.app.ui.ExportSettingsActions
import me.teyvat.app.ui.ExportSettingsScreen
import me.teyvat.app.ui.HomeActions
import me.teyvat.app.ui.HomeContent
import me.teyvat.app.ui.HomeTopBar
import me.teyvat.app.ui.LicencesScreen
import me.teyvat.app.ui.SettingsActions
import me.teyvat.app.ui.SettingsScreen
import me.teyvat.app.ui.SetUpActions
import me.teyvat.app.ui.SetUpScreen
import me.teyvat.app.ui.TeyvatActions
import me.teyvat.app.ui.TeyvatScreen
import me.teyvat.app.ui.TrackerView
import me.teyvat.app.ui.TeyvatTheme
import me.teyvat.app.ui.ThanksScreen
import me.teyvat.app.ui.Tm
import me.teyvat.app.ui.TmIcon
import me.teyvat.app.ui.TmTopBar
import me.teyvat.app.ui.WelcomeScreen

enum class Screen { Welcome, SetUp, Home, Settings, ExportSettings, Teyvat, Activity, About, Thanks, Licences, Notices }

/** The one activity: first run, Home and the pushed screens (spec section 3). */
class MainActivity : ComponentActivity(), HomeActions, SetUpActions, SettingsActions, ExportSettingsActions, TeyvatActions {
    private val app get() = application as TeyvatApp
    private val prefs get() = app.prefs
    private val capture get() = app.capture
    private val tracker get() = app.tracker
    private val updater get() = app.updater
    private var whoAt = 0L
    private val snackbars = SnackbarHostState()
    private val stack = mutableStateListOf<Screen>()
    private var notificationsGranted by mutableStateOf(true)
    /** Unrestricted battery use (Battery.kt), read again on every resume. */
    private var backgroundAllowed by mutableStateOf(true)
    /** files/crash.txt is newer than the last Dismiss. */
    private var crashed by mutableStateOf(false)
    /** The JSON a Save is waiting to write, built when Save was tapped. */
    private var pendingSave: String? = null
    /** Start capture asked for notifications first; the VPN comes next. */
    private var startAfterPermission = false

    private val vpnConsent = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        if (result.resultCode == RESULT_OK) {
            vpnAllowed()
        } else {
            capture.setVpn(VpnState.Off)
            snack(getString(R.string.vpn_not_allowed), getString(R.string.try_again)) { startCapture() }
        }
    }

    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        notificationsGranted = notificationsAllowed()
        if (startAfterPermission) {
            startAfterPermission = false
            askForVpn()
        }
    }

    private val saveDocument = registerForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        val json = pendingSave
        pendingSave = null
        if (uri == null || json == null) return@registerForActivityResult
        lifecycleScope.launch {
            val ok = withContext(Dispatchers.IO) { runCatching { Exports.write(this@MainActivity, uri, json) }.isSuccess }
            if (ok) {
                prefs.update { it.copy(lastSave = uri.toString()) }
                val name = Exports.displayName(this@MainActivity, uri) ?: Exports.fileName()
                NativeBridge.note("ok", getString(R.string.export_saved, name))
                snack(getString(R.string.export_saved, name), getString(R.string.open)) { Exports.openFile(this@MainActivity, uri) }
            } else {
                snack(getString(R.string.export_save_failed))
            }
        }
    }

    /** Where the wish history read goes when it finishes: a picked file, the share sheet. */
    private var pendingWishSave: Uri? = null
    private var pendingWishShare = false

    private val saveWishDocument = registerForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        if (uri == null) return@registerForActivityResult
        pendingWishSave = uri
        if (!wishFileFresh()) startWishRead()
        onWishRead(capture.summary.value.wish.read)
    }

    private val chooseTree = registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        if (uri == null) return@registerForActivityResult
        contentResolver.takePersistableUriPermission(
            uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
        )
        prefs.update { it.copy(exportTree = uri.toString(), autoExport = true, lastAutoExport = null) }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        GameVpnService.createChannels(this)
        notificationsGranted = notificationsAllowed()
        if (stack.isEmpty()) stack.add(if (prefs.value.onboarded) Screen.Home else Screen.Welcome)
        handle(intent)
        setContent {
            val settings by prefs.settings.collectAsState()
            val dark = when (settings.theme) {
                ThemeMode.System -> isSystemInDarkTheme()
                ThemeMode.Light -> false
                ThemeMode.Dark -> true
            }
            TeyvatTheme(dark) { Root(settings, dark) }
        }
    }

    override fun onPause() {
        super.onPause()
        Updater.foreground = false
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    override fun onResume() {
        super.onResume()
        Clock.refresh(this)
        Updater.foreground = true
        updater.onResume()
        // Back from Settings › Install unknown apps: carry on.
        if (updater.state.value is UpdateState.NeedsPermission && packageManager.canRequestPackageInstalls()) {
            updater.installNow()
        }
        notificationsGranted = notificationsAllowed()
        backgroundAllowed = Battery.unrestricted(this)
        crashed = crashFile().let { it.exists() && it.lastModified() > getSharedPreferences("settings", MODE_PRIVATE).getLong("crash_dismissed", 0) }
        if (stack.lastOrNull() == Screen.Home) tracker.todayShown()
        capture.refresh()
        // The upload target is changed on the website: read it again on return.
        if (System.currentTimeMillis() - whoAt > 10_000) {
            whoAt = System.currentTimeMillis()
            tracker.refreshWho()
        }
    }

    /** The notification's actions and the alert's Start capture. */
    private fun handle(intent: Intent?) {
        when (intent?.action) {
            ACTION_SHARE_GOOD -> shareGood()
            ACTION_START_CAPTURE -> {
                openGameAfterStart = intent.getBooleanExtra(EXTRA_OPEN_GAME, false)
                startCapture()
            }
            ACTION_INSTALL_UPDATE -> updater.installNow()
            else -> return
        }
        intent.action = null
        if (stack.lastOrNull() != Screen.Home && prefs.value.onboarded) {
            stack.clear()
            stack.add(Screen.Home)
        }
    }

    private fun push(screen: Screen) {
        stack.add(screen)
    }

    private fun pop() {
        if (stack.size > 1) stack.removeAt(stack.lastIndex) else finish()
    }

    @Composable
    private fun Root(settings: Settings, dark: Boolean) {
        val screen = stack.lastOrNull() ?: Screen.Home
        BackHandler(enabled = stack.size > 1) { pop() }
        val vpn by capture.vpn.collectAsState()
        val summary by capture.summary.collectAsState()
        androidx.compose.runtime.LaunchedEffect(summary.wish.read) { onWishRead(summary.wish.read) }
        val hero = hero(vpn, summary)
        val folder = Exports.folderLabel(this, settings)
        val trackerState by tracker.state.collectAsState()
        val lastUpload by tracker.lastUpload.collectAsState()
        val uploading by tracker.uploading.collectAsState()
        val revoked by tracker.revoked.collectAsState()
        val importKey by tracker.importKey.collectAsState()
        val lastLive by tracker.lastLive.collectAsState()
        val signInUrl by tracker.signInUrl.collectAsState()
        val plan by tracker.plan.collectAsState()
        val trackerAlerts by tracker.alerts.collectAsState()
        // Today (on Home) came into sight: its plan is read again.
        androidx.compose.runtime.LaunchedEffect(screen) { if (screen == Screen.Home) tracker.todayShown() }
        // whoami every 5 min too (the desktop's), not only on resume: the
        // upload target may change on the website while the app is open.
        androidx.compose.runtime.LaunchedEffect(Unit) {
            while (true) {
                kotlinx.coroutines.delay(5 * 60_000L)
                tracker.refreshWho()
            }
        }
        val trackerView = TrackerView(
            trackerState, lastUpload, uploading, settings.trackerUpload, settings.sendWishes, settings.liveSync,
            revoked = revoked, capturedUid = summary.player?.uid, key = importKey, api = Links.trackerApi,
            lastLive = lastLive, signInUrl = signInUrl, plan = plan, alerts = trackerAlerts,
        )
        val update by updater.state.collectAsState()
        Scaffold(
            containerColor = MaterialTheme.colorScheme.background,
            topBar = {
                // Opaque: a list scrolled up passes under the bar, not through it
                // (a short window, a pop-up view, a big font).
                Box(Modifier.background(MaterialTheme.colorScheme.background).windowInsetsPadding(WindowInsets.statusBars)) {
                    when (screen) {
                        Screen.Welcome -> {}
                        Screen.SetUp -> TmTopBar(onBack = ::pop, title = "")
                        Screen.Home -> HomeTopBar(
                            error = activityLine(summary)?.isError == true, dark = dark,
                            onHistory = ::openActivity, onTheme = ::cycleTheme, onSettings = ::openSettings,
                            onDiscord = { openUrl(Links.DISCORD) },
                        )
                        Screen.Activity -> TmTopBar(stringResource(R.string.activity), ::pop) {
                            IconButton(onClick = ::shareLog) { TmIcon(R.drawable.ic_share, stringResource(R.string.share_log)) }
                        }
                        Screen.Settings -> TmTopBar(stringResource(R.string.settings), ::pop)
                        Screen.ExportSettings -> TmTopBar(stringResource(R.string.export_title), ::pop)
                        Screen.Teyvat -> TmTopBar(Links.trackerHost, ::pop) {
                            (trackerState as? TrackerState.SignedIn)?.let { signedIn ->
                                IconButton(onClick = ::refreshTracker) { TmIcon(R.drawable.ic_refresh, stringResource(R.string.refresh)) }
                                IconButton(onClick = { openUrl(signedIn.who.dashboardUrl) }) {
                                    TmIcon(R.drawable.ic_external_link, stringResource(R.string.tracker_dashboard))
                                }
                            }
                        }
                        Screen.About -> TmTopBar(stringResource(R.string.about), ::pop)
                        Screen.Thanks -> TmTopBar(stringResource(R.string.thanks), ::pop)
                        Screen.Licences -> TmTopBar(stringResource(R.string.licences), ::pop)
                        Screen.Notices -> TmTopBar(stringResource(R.string.third_party_notices), ::pop)
                    }
                }
            },
            snackbarHost = {
                SnackbarHost(snackbars) { data ->
                    Snackbar(
                        data,
                        containerColor = MaterialTheme.colorScheme.inverseSurface,
                        contentColor = MaterialTheme.colorScheme.inverseOnSurface,
                        actionColor = MaterialTheme.colorScheme.inversePrimary,
                    )
                }
            },
        ) { padding ->
            val bottomOnly = PaddingValues(top = padding.calculateTopPadding(), bottom = padding.calculateBottomPadding())
            when (screen) {
                Screen.Welcome -> WelcomeScreen(bottomOnly) { push(Screen.SetUp) }
                Screen.SetUp -> SetUpScreen(bottomOnly, settings, folder, notificationsGranted, backgroundAllowed, this)
                Screen.Home -> HomeContent(hero, summary, settings, capture.startingSince, folder, trackerView, update, backgroundAllowed, crashed, this, padding)
                Screen.Settings -> SettingsScreen(padding, settings, trackerView, update, backgroundAllowed, this)
                Screen.ExportSettings -> ExportSettingsScreen(padding, settings, folder, this)
                Screen.Teyvat -> TeyvatScreen(padding, trackerView, summary.captured, settings, this)
                Screen.Activity -> ActivityScreen(padding, summary.events)
                Screen.About -> AboutScreen(padding, BuildConfig.VERSION_NAME, gameDataLabel(summary), ::openUrl, {
                    me.teyvat.app.ui.UpdateBannerFor(update, this, Modifier.padding(horizontal = 16.dp), details = false)
                }) { push(Screen.Thanks) }
                Screen.Thanks -> ThanksScreen(padding) { push(Screen.Licences) }
                Screen.Licences -> LicencesScreen(padding, ::openUrl) { push(Screen.Notices) }
                Screen.Notices -> me.teyvat.app.ui.NoticesScreen(padding)
            }
        }
    }

    private fun snack(message: String, action: String? = null, onAction: (() -> Unit)? = null) {
        lifecycleScope.launch {
            snackbars.currentSnackbarData?.dismiss()
            val result = snackbars.showSnackbar(message, action)
            if (result == SnackbarResult.ActionPerformed) onAction?.invoke()
        }
    }

    // -- capture ------------------------------------------------------------

    override fun startCapture() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && !notificationsAllowed() && !askedForNotifications) {
            askedForNotifications = true
            startAfterPermission = true
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
            return
        }
        askForVpn()
    }

    private var askedForNotifications = false

    override fun stopCapture() {
        GameVpnService.stop(this)
    }

    private fun askForVpn() {
        val consent = VpnService.prepare(this)
        if (consent != null) {
            try {
                vpnConsent.launch(consent)
            } catch (_: ActivityNotFoundException) {
                snack(getString(R.string.vpn_not_allowed))
            }
        } else {
            vpnAllowed()
        }
    }

    private fun vpnAllowed() {
        if (!prefs.value.onboarded) {
            prefs.update { it.copy(onboarded = true) }
            stack.clear()
            stack.add(Screen.Home)
        }
        capture.setVpn(VpnState.Starting)
        GameVpnService.start(this)
        if (openGameAfterStart) {
            openGameAfterStart = false
            // The game must connect after the VPN is up, for its login's key.
            lifecycleScope.launch {
                val up = withTimeoutOrNull(OPEN_GAME_WAIT_MS) { capture.vpn.first { it == VpnState.On } }
                if (up != null) openGenshin()
            }
        }
    }

    /** Start capture came from the tile with "Tile opens Genshin" on: the game opens once the VPN is up. */
    private var openGameAfterStart = false

    override fun gameInstalled() = Links.gameLaunch(this) != null

    override fun openGenshin() {
        Links.gameLaunch(this)?.let(::startActivity)
    }

    override fun forceStopGenshin() {
        runCatching { startActivity(Links.gameAppInfo()) }
    }

    // -- navigation -----------------------------------------------------------

    override fun openActivity() = push(Screen.Activity)
    override fun openSettings() = push(Screen.Settings)
    override fun openExportSettings() = push(Screen.ExportSettings)
    override fun openTeyvat() = push(Screen.Teyvat)
    override fun openAbout() = push(Screen.About)

    override fun openUrl(url: String) {
        try {
            CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(this, Uri.parse(url))
        } catch (_: ActivityNotFoundException) {
            runCatching { startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url))) }
        }
    }

    // -- updates ------------------------------------------------------------

    override fun updateInstallNow() {
        when (updater.state.value) {
            is UpdateState.Failed -> updater.check()
            is UpdateState.NeedsPermission -> updateAllow()
            else -> updater.installNow()
        }
    }

    override fun updateAllow() {
        runCatching { startActivity(updater.permissionIntent()) }
    }

    override fun setTileOpensGame(on: Boolean) {
        prefs.update { it.copy(tileOpensGame = on) }
    }

    override fun setResinAlert(on: Boolean) {
        prefs.update { it.copy(resinAlert = on) }
        if (on) askForNotifications()
    }

    override fun setResinAlertAt(value: Int) {
        prefs.update { it.copy(resinAlertAt = value.coerceIn(1, Today.RESIN_MAX)) }
    }

    override fun testResinAlert() {
        askForNotifications()
        // As the real one reads ("Full at …"), not "Resin is full" while it isn't.
        ResinAlert.show(this, prefs.value.resinAlertAt, capture.summary.value.today?.resinFullAt() ?: 0L)
    }

    /** Android 13+: notifications need the user's yes. */
    private fun askForNotifications() {
        if (android.os.Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(android.Manifest.permission.POST_NOTIFICATIONS), 33)
        }
    }

    override fun setAutoUpdate(on: Boolean) {
        prefs.update { it.copy(autoUpdate = on) }
        if (on) updater.check(manual = false)
    }

    override fun checkForUpdates() = updater.check()

    // -- tracker ------------------------------------------------------------

    override fun trackerSignIn() {
        tracker.signIn(::openUrl) { error ->
            // Back from the browser: closes the Custom Tab above this activity.
            startActivity(
                Intent(this, MainActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            )
            if (error == null) {
                // Connecting turns uploads on (spec 7).
                prefs.update { it.copy(trackerUpload = true) }
                snack(getString(R.string.tracker_signed_in_toast, Links.trackerHost))
                tracker.uploadNow(capture.summary.value)
            } else {
                snack(getString(R.string.tracker_sign_in_failed, error))
            }
        }
    }

    override fun trackerCancelSignIn() = tracker.cancelSignIn()

    override fun copySignInLink() {
        val url = tracker.signInUrl.value ?: return
        Exports.copy(this, "Sign-in link", url, sensitive = true)
        snack(getString(R.string.link_copied))
    }

    /** whoami again (the upload target is chosen on the website). */
    private fun refreshTracker() {
        whoAt = System.currentTimeMillis()
        tracker.refreshWho()
    }

    /** The top bar's Theme button: System → Light → Dark, and a snackbar names the new one. */
    private fun cycleTheme() {
        val next = when (prefs.value.theme) {
            ThemeMode.System -> ThemeMode.Light
            ThemeMode.Light -> ThemeMode.Dark
            ThemeMode.Dark -> ThemeMode.System
        }
        setTheme(next)
        val name = getString(
            when (next) {
                ThemeMode.System -> R.string.theme_system
                ThemeMode.Light -> R.string.theme_light
                ThemeMode.Dark -> R.string.theme_dark
            },
        )
        snack(getString(R.string.theme_now, name))
    }

    override fun trackerUploadNow() {
        if (!tracker.uploadNow(capture.summary.value)) snack(getString(R.string.tracker_nothing_to_upload))
    }

    override fun setTrackerUpload(on: Boolean) {
        prefs.update { it.copy(trackerUpload = on) }
    }

    override fun setLiveSync(on: Boolean) {
        prefs.update { it.copy(liveSync = on) }
    }

    override fun setSendWishes(on: Boolean) {
        prefs.update { it.copy(sendWishes = on) }
    }

    override fun trackerSignOut() = tracker.signOut()

    override fun setImportKey(key: String) = tracker.setImportKey(key)

    override fun clearImportKey() = tracker.clearImportKey()

    override fun setTrackerApi(api: String) = tracker.setApi(api)

    // -- export -------------------------------------------------------------

    /** The GOOD JSON for a user action, or a snackbar saying why not. */
    private fun goodOrSay(then: (String) -> Unit) {
        val settings = prefs.value
        when (val problem = Exports.problem(capture.summary.value, settings.filters)) {
            ExportProblem.NothingYet -> return snack(getString(R.string.export_nothing))
            is ExportProblem.Missing -> return snack(getString(R.string.export_not_captured, problem.what.joinToString(", ") { getString(it) }))
            null -> {}
        }
        lifecycleScope.launch {
            val json = withContext(Dispatchers.Default) { Exports.good(settings.filters) }
            if (json == null) snack(getString(R.string.export_nothing)) else then(json)
        }
    }

    override fun saveGood() = goodOrSay { json ->
        pendingSave = json
        saveDocument.launch(Exports.fileName())
    }

    override fun shareGood() = goodOrSay { json ->
        lifecycleScope.launch {
            val intent = withContext(Dispatchers.IO) { Exports.shareIntent(this@MainActivity, json) }
            startActivity(intent)
        }
    }

    override fun copyGood() = goodOrSay { json ->
        if (Exports.copy(this, "${BuildConfig.FORMAT_NAME} JSON", json)) {
            snack(getString(R.string.export_copied, BuildConfig.FORMAT_NAME))
        } else {
            snack(getString(R.string.export_too_big))
        }
    }

    override fun openExportFolder() {
        val settings = prefs.value
        if (!settings.autoExport && settings.lastSave != null) {
            if (Exports.openFile(this, Uri.parse(settings.lastSave))) return
        }
        if (!Exports.openFolder(this, settings)) snack(getString(R.string.export_no_folder))
    }

    override fun setAutoExport(on: Boolean) {
        prefs.update { it.copy(autoExport = on) }
    }

    override fun chooseFolder() {
        runCatching { chooseTree.launch(null) }
    }

    override fun useDownloads() {
        prefs.value.exportTree?.let { tree ->
            runCatching {
                contentResolver.releasePersistableUriPermission(
                    Uri.parse(tree), Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
                )
            }
        }
        prefs.update { it.copy(exportTree = null, lastAutoExport = null) }
    }

    override fun setFilters(filters: ExportFilters) {
        prefs.update { it.copy(filters = filters) }
    }

    /** Copy achievements; with [openUrl], open StarDB or Seelie.me after. */
    // -- wish history ---------------------------------------------------------

    override fun copyWishUrl(openUrl: String?) {
        val url = NativeBridge.wishUrl() ?: return snack(getString(R.string.wish_no_key))
        Exports.copy(this, "Wish URL", url, sensitive = true)
        snack(getString(R.string.wish_url_copied))
        if (openUrl != null) openUrl(openUrl)
    }

    /** A finished read is reused for a minute, then read again (new wishes). */
    private fun wishFileFresh(): Boolean {
        val read = capture.summary.value.wish.read
        return read.state == "done" && System.currentTimeMillis() - read.at < 60_000
    }

    private fun startWishRead() {
        when (NativeBridge.startWishHistory(BuildConfig.VERSION_NAME)) {
            0 -> capture.refresh()
            1 -> snack(getString(R.string.wish_no_key))
            2 -> snack(getString(R.string.wish_waiting_uid))
            3 -> snack(getString(R.string.wish_busy))
        }
        if (capture.summary.value.wish.read.state != "running") {
            pendingWishSave = null
            pendingWishShare = false
        }
    }

    override fun saveWishes() {
        runCatching { saveWishDocument.launch(Exports.wishFileName()) }
    }

    override fun shareWishes() {
        pendingWishShare = true
        if (!wishFileFresh()) startWishRead()
        onWishRead(capture.summary.value.wish.read)
    }

    override fun cancelWishes() {
        NativeBridge.cancelWishHistory()
        pendingWishSave = null
        pendingWishShare = false
    }

    /** The read moved: finish a pending Save or Share, or say why not. */
    private fun onWishRead(read: WishRead) {
        if (pendingWishSave == null && !pendingWishShare) return
        when (read.state) {
            "done" -> {
                val json = NativeBridge.wishHistory() ?: return
                pendingWishSave?.let { uri ->
                    pendingWishSave = null
                    lifecycleScope.launch {
                        val ok = withContext(Dispatchers.IO) { runCatching { Exports.write(this@MainActivity, uri, json) }.isSuccess }
                        val name = Exports.displayName(this@MainActivity, uri) ?: Exports.wishFileName()
                        if (ok) {
                            NativeBridge.note("ok", getString(R.string.wish_saved, name))
                            snack(getString(R.string.wish_saved, name), getString(R.string.open)) { Exports.openFile(this@MainActivity, uri) }
                        } else {
                            snack(getString(R.string.export_save_failed))
                        }
                    }
                }
                if (pendingWishShare) {
                    pendingWishShare = false
                    lifecycleScope.launch {
                        val intent = withContext(Dispatchers.IO) {
                            Exports.shareIntent(this@MainActivity, json, Exports.wishFileName())
                        }
                        startActivity(intent)
                    }
                }
            }
            "failed" -> {
                pendingWishSave = null
                pendingWishShare = false
                snack(getString(R.string.wish_read_failed, read.error.orEmpty()))
            }
            "idle" -> {
                pendingWishSave = null
                pendingWishShare = false
            }
        }
    }

    override fun copyAchievements(openUrl: String?) {
        val json = NativeBridge.achievements()
        if (json == null) {
            if (openUrl != null) {
                snack(getString(R.string.achievements_not_found), getString(R.string.open_anyway)) { openUrl(openUrl) }
            } else {
                snack(getString(R.string.achievements_not_found))
            }
            return
        }
        Exports.copy(this, "Achievements", json)
        snack(getString(R.string.achievements_copied, capture.summary.value.counts.achievements))
        if (openUrl != null) openUrl(openUrl)
    }

    private fun crashFile() = java.io.File(filesDir, "crash.txt")

    override fun dismissCrash() {
        getSharedPreferences("settings", MODE_PRIVATE).edit().putLong("crash_dismissed", crashFile().lastModified()).apply()
        crashed = false
    }

    override fun setLanguage(tag: String?) = AppLocale.set(this, tag)

    override fun attachBaseContext(newBase: android.content.Context) {
        super.attachBaseContext(AppLocale.wrap(newBase))
    }

    override fun shareLog() {
        val intent = Exports.shareLogIntent(this) ?: return snack(getString(R.string.no_log))
        startActivity(intent)
    }

    // -- set up and settings ------------------------------------------------

    override fun openVpnSettings() {
        runCatching { startActivity(Intent(AndroidSettings.ACTION_VPN_SETTINGS)) }
    }

    override fun openAlwaysOnVpn() = openVpnSettings()

    override fun notificationsAllowed(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    override fun allowNotifications() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            askedForNotifications = true
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    override fun openNotificationSettings() {
        runCatching {
            startActivity(Intent(AndroidSettings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(AndroidSettings.EXTRA_APP_PACKAGE, packageName))
        }
    }

    override fun allowBackground() = Battery.ask(this)

    override fun setTheme(theme: ThemeMode) {
        prefs.update { it.copy(theme = theme) }
    }

    override fun setRecordCapture(on: Boolean) {
        prefs.update { it.copy(recordCapture = on) }
    }

    companion object {
        const val ACTION_SHARE_GOOD = "me.teyvat.app.SHARE_GOOD"
        const val ACTION_START_CAPTURE = "me.teyvat.app.START_CAPTURE"
        const val ACTION_INSTALL_UPDATE = "me.teyvat.app.INSTALL_UPDATE"
        const val EXTRA_OPEN_GAME = "open_game"

        /** How long the tile's Start waits for the VPN before giving up on opening the game. */
        private const val OPEN_GAME_WAIT_MS = 15_000L

    }
}
