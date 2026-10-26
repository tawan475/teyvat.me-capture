// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import me.teyvat.app.ActivityEvent
import me.teyvat.app.ExportFilters
import me.teyvat.app.Links
import me.teyvat.app.R
import me.teyvat.app.Settings
import me.teyvat.app.ThemeMode
import me.teyvat.app.TrackerState
import me.teyvat.app.UpdateState
import me.teyvat.app.hhmm

@Composable
private fun Scrolling(padding: PaddingValues, content: @Composable () -> Unit) {
    Column(
        Modifier
            .fillMaxSize()
            .padding(padding)
            .verticalScroll(rememberScrollState())
            .padding(bottom = 24.dp),
    ) { content() }
}

@Composable
private fun Group(content: @Composable () -> Unit) {
    Column(Modifier.padding(horizontal = 16.dp)) { TmCard { content() } }
}

@Composable
private fun Chevron() = TmIcon(R.drawable.ic_chevron_right, null)

@Composable
private fun External() = TmIcon(R.drawable.ic_external_link, null, size = 18.dp)

interface SettingsActions {
    fun openAlwaysOnVpn()
    fun openNotificationSettings()
    fun setTheme(theme: ThemeMode)
    fun openExportSettings()
    fun openTeyvat()
    fun openActivity()
    fun setRecordCapture(on: Boolean)
    fun openAbout()
    fun setAutoUpdate(on: Boolean)
    fun checkForUpdates()
}

@Composable
fun SettingsScreen(padding: PaddingValues, settings: Settings, tracker: TrackerView, update: UpdateState, actions: SettingsActions) = Scrolling(padding) {
    SectionLabel(stringResource(R.string.section_app))
    Group {
        ListRow(stringResource(R.string.always_on_vpn), stringResource(R.string.always_on_vpn_line), R.drawable.ic_power, onClick = actions::openAlwaysOnVpn) { External() }
        CardDivider()
        ListRow(stringResource(R.string.notifications), stringResource(R.string.notifications_line), R.drawable.ic_bell, onClick = actions::openNotificationSettings) { External() }
        CardDivider()
        Column(Modifier.padding(16.dp)) {
            Text(stringResource(R.string.theme), style = MaterialTheme.typography.bodyLarge)
            Spacer(Modifier.height(10.dp))
            val options = listOf(
                ThemeMode.System to R.string.theme_system, ThemeMode.Light to R.string.theme_light, ThemeMode.Dark to R.string.theme_dark,
            )
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                options.forEachIndexed { i, (mode, label) ->
                    SegmentedButton(
                        selected = settings.theme == mode,
                        onClick = { actions.setTheme(mode) },
                        shape = SegmentedButtonDefaults.itemShape(i, options.size),
                        colors = SegmentedButtonDefaults.colors(
                            activeContainerColor = MaterialTheme.colorScheme.primaryContainer,
                            activeContentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                        ),
                    ) { Text(stringResource(label)) }
                }
            }
        }
    }
    SectionLabel(stringResource(R.string.section_export))
    Group {
        ListRow(stringResource(R.string.export_filters), null, R.drawable.ic_sliders, onClick = actions::openExportSettings) { Chevron() }
    }
    SectionLabel(stringResource(R.string.section_tracker))
    Group {
        val line = when (val state = tracker.state) {
            TrackerState.SignedOut -> stringResource(R.string.tracker_signed_out_line)
            TrackerState.SigningIn -> stringResource(R.string.tracker_signing_in)
            is TrackerState.SignedIn -> stringResource(R.string.tracker_signed_in_as, state.who.user)
        }
        ListRow(Links.trackerHost, line, R.drawable.ic_cloud_upload, onClick = actions::openTeyvat) { Chevron() }
    }
    SectionLabel(stringResource(R.string.section_updates))
    Group {
        SwitchRow(
            stringResource(R.string.update_auto), stringResource(R.string.update_auto_line), settings.autoUpdate,
            actions::setAutoUpdate, icon = R.drawable.ic_download,
        )
        CardDivider()
        val line = when (update) {
            UpdateState.Checking -> stringResource(R.string.update_checking)
            UpdateState.UpToDate -> stringResource(R.string.update_up_to_date, me.teyvat.app.BuildConfig.VERSION_NAME)
            is UpdateState.Downloading -> stringResource(R.string.update_downloading, update.version, update.percent)
            is UpdateState.Ready -> stringResource(R.string.update_ready, update.version)
            is UpdateState.NeedsPermission -> stringResource(R.string.update_allow_line)
            is UpdateState.Installing -> stringResource(R.string.update_installing, update.version)
            is UpdateState.Failed -> stringResource(R.string.update_failed, update.error)
            UpdateState.Idle -> me.teyvat.app.BuildConfig.VERSION_NAME
        }
        ListRow(stringResource(R.string.update_check), line, R.drawable.ic_clock, onClick = actions::checkForUpdates)
    }
    SectionLabel(stringResource(R.string.section_diagnostics))
    Group {
        ListRow(stringResource(R.string.activity), null, R.drawable.ic_clock, onClick = actions::openActivity) { Chevron() }
        CardDivider()
        SwitchRow(
            stringResource(R.string.record_capture), stringResource(R.string.record_capture_line), settings.recordCapture,
            actions::setRecordCapture, icon = R.drawable.ic_bug,
        )
        CardDivider()
        ListRow(stringResource(R.string.about), null, R.drawable.ic_info, onClick = actions::openAbout) { Chevron() }
    }
}

interface ExportSettingsActions {
    fun setAutoExport(on: Boolean)
    fun chooseFolder()
    fun useDownloads()
    fun setFilters(filters: ExportFilters)
}

/** `−  value  +` for one minimum. */
@Composable
private fun MinStepper(label: String, value: Int, range: IntRange, format: (Int) -> String, onChange: (Int) -> Unit) {
    Row(Modifier.fillMaxWidth().padding(start = 56.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
        IconButton(onClick = { onChange((value - 1).coerceIn(range)) }, enabled = value > range.first) {
            Text("−", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary)
        }
        Text(format(value), style = MaterialTheme.typography.bodyLarge, textAlign = TextAlign.Center, modifier = Modifier.widthIn(min = 40.dp))
        IconButton(onClick = { onChange((value + 1).coerceIn(range)) }, enabled = value < range.last) {
            Text("+", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary)
        }
    }
}

fun characterSummary(f: ExportFilters) = "Lv ${f.minCharacterLevel}+ · C${f.minCharacterConstellation}+ · A${f.minCharacterAscension}+"
fun weaponSummary(f: ExportFilters) =
    "Lv ${f.minWeaponLevel}+ · R${f.minWeaponRefinement.coerceAtLeast(1)}+ · A${f.minWeaponAscension}+ · ${f.minWeaponRarity}★+"
fun artifactSummary(f: ExportFilters) = "+${f.minArtifactLevel}+ · ${f.minArtifactRarity}★+"

@Composable
fun ExportSettingsScreen(padding: PaddingValues, settings: Settings, folderLabel: String, actions: ExportSettingsActions) = Scrolling(padding) {
    val f = settings.filters
    var open by remember { mutableStateOf<String?>(null) }
    fun toggle(key: String) { open = if (open == key) null else key }
    SectionLabel(stringResource(R.string.export_auto_section))
    Group {
        SwitchRow(
            stringResource(R.string.export_auto_switch), stringResource(R.string.export_auto_switch_line), settings.autoExport,
            actions::setAutoExport, icon = R.drawable.ic_folder,
        )
        CardDivider()
        ListRow(folderLabel, stringResource(R.string.export_choose_another), onClick = actions::chooseFolder) { Chevron() }
        if (settings.exportTree != null) {
            CardDivider()
            ListRow(stringResource(R.string.export_use_downloads), null, onClick = actions::useDownloads)
        }
    }
    SectionLabel(stringResource(R.string.filters))
    Group {
        @Composable
        fun FilterRow(key: String, title: String, line: String, on: Boolean, set: (Boolean) -> Unit, steppers: (@Composable () -> Unit)?) {
            ListRow(title, line, onClick = if (steppers != null) ({ toggle(key) }) else null) {
                TmSwitch(on, set)
            }
            if (open == key && steppers != null) {
                steppers()
                Spacer(Modifier.height(8.dp))
            }
        }
        FilterRow("c", stringResource(R.string.tile_characters), characterSummary(f), f.includeCharacters, { actions.setFilters(f.copy(includeCharacters = it)) }) {
            MinStepper(stringResource(R.string.min_level), f.minCharacterLevel, 1..90, { "$it" }) { actions.setFilters(f.copy(minCharacterLevel = it)) }
            MinStepper(stringResource(R.string.min_constellation), f.minCharacterConstellation, 0..6, { "C$it" }) { actions.setFilters(f.copy(minCharacterConstellation = it)) }
            MinStepper(stringResource(R.string.min_ascension), f.minCharacterAscension, 0..6, { "A$it" }) { actions.setFilters(f.copy(minCharacterAscension = it)) }
        }
        CardDivider()
        FilterRow("w", stringResource(R.string.tile_weapons), weaponSummary(f), f.includeWeapons, { actions.setFilters(f.copy(includeWeapons = it)) }) {
            MinStepper(stringResource(R.string.min_level), f.minWeaponLevel, 1..90, { "$it" }) { actions.setFilters(f.copy(minWeaponLevel = it)) }
            MinStepper(stringResource(R.string.min_refinement), f.minWeaponRefinement.coerceAtLeast(1), 1..5, { "R$it" }) { actions.setFilters(f.copy(minWeaponRefinement = it)) }
            MinStepper(stringResource(R.string.min_ascension), f.minWeaponAscension, 0..6, { "A$it" }) { actions.setFilters(f.copy(minWeaponAscension = it)) }
            MinStepper(stringResource(R.string.min_rarity), f.minWeaponRarity, 1..5, { "$it★" }) { actions.setFilters(f.copy(minWeaponRarity = it)) }
        }
        CardDivider()
        FilterRow("a", stringResource(R.string.tile_artifacts), artifactSummary(f), f.includeArtifacts, { actions.setFilters(f.copy(includeArtifacts = it)) }) {
            MinStepper(stringResource(R.string.min_level), f.minArtifactLevel, 0..20, { "+$it" }) { actions.setFilters(f.copy(minArtifactLevel = it)) }
            MinStepper(stringResource(R.string.min_rarity), f.minArtifactRarity, 1..5, { "$it★" }) { actions.setFilters(f.copy(minArtifactRarity = it)) }
        }
        CardDivider()
        FilterRow("m", stringResource(R.string.tile_materials), stringResource(R.string.filter_all), f.includeMaterials, { actions.setFilters(f.copy(includeMaterials = it)) }, null)
        CardDivider()
        SwitchRow(
            stringResource(R.string.filter_4th_line), stringResource(R.string.filter_4th_line_line), f.fakeInitialize4thLine,
            { actions.setFilters(f.copy(fakeInitialize4thLine = it)) },
        )
    }
    Text(
        stringResource(R.string.filters_footnote), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline,
        modifier = Modifier.padding(horizontal = 32.dp, vertical = 12.dp),
    )
}

interface TeyvatActions {
    fun trackerSignIn()
    fun trackerCancelSignIn()
    fun trackerUploadNow()
    fun setTrackerUpload(on: Boolean)
    fun trackerSignOut()
    fun openUrl(url: String)
}

/** teyvat.me: sign in, where uploads go, upload now, sign out. */
@Composable
fun TeyvatScreen(padding: PaddingValues, tracker: TrackerView, captured: Boolean, actions: TeyvatActions) = Scrolling(padding) {
    Spacer(Modifier.height(8.dp))
    val state = tracker.state
    Group {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Avatar(icon = R.drawable.ic_user)
            Spacer(Modifier.width(16.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    if (state is TrackerState.SignedIn) stringResource(R.string.tracker_signed_in_as, state.who.user)
                    else stringResource(R.string.teyvat_not_connected),
                    style = MaterialTheme.typography.bodyLarge,
                )
                Text(
                    when (state) {
                        TrackerState.SignedOut -> stringResource(R.string.teyvat_browser_line)
                        TrackerState.SigningIn -> stringResource(R.string.tracker_signing_in)
                        is TrackerState.SignedIn -> targetLine(state.who)
                    },
                    style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Column(Modifier.padding(start = 16.dp, end = 16.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            when (state) {
                TrackerState.SignedOut -> TmButton(stringResource(R.string.tracker_sign_in), actions::trackerSignIn, large = true)
                TrackerState.SigningIn -> TmButton(stringResource(R.string.cancel), actions::trackerCancelSignIn, kind = ButtonKind.Outlined, large = true)
                is TrackerState.SignedIn -> {
                    TmButton(
                        stringResource(if (tracker.uploading) R.string.tracker_uploading else R.string.tracker_upload_now),
                        actions::trackerUploadNow, large = true, enabled = captured && !tracker.uploading,
                    )
                    tracker.last?.let { last ->
                        Text(
                            last.text, style = MaterialTheme.typography.bodySmall,
                            color = if (last.ok) MaterialTheme.colorScheme.outline else MaterialTheme.colorScheme.error,
                        )
                    }
                }
            }
        }
        if (state is TrackerState.SignedIn) {
            CardDivider()
            SwitchRow(
                stringResource(R.string.tracker_upload_switch), stringResource(R.string.tracker_upload_switch_line), tracker.uploadOn,
                actions::setTrackerUpload, icon = R.drawable.ic_cloud_upload,
            )
            CardDivider()
            ListRow(
                stringResource(R.string.tracker_configure), stringResource(R.string.tracker_configure_line), R.drawable.ic_sliders,
                onClick = { actions.openUrl(state.who.configUrl) },
            ) { External() }
            CardDivider()
            ListRow(stringResource(R.string.tracker_dashboard), null, R.drawable.ic_globe, onClick = { actions.openUrl(state.who.dashboardUrl) }) { External() }
            CardDivider()
            ListRow(stringResource(R.string.tracker_sign_out), null, R.drawable.ic_log_in, onClick = actions::trackerSignOut)
        } else {
            CardDivider()
            ListRow(Links.trackerHost, stringResource(R.string.teyvat_site_line), R.drawable.ic_globe, onClick = { actions.openUrl(Links.trackerSite) }) { External() }
        }
    }
}

@Composable
fun ActivityScreen(padding: PaddingValues, events: List<ActivityEvent>) {
    LazyColumn(Modifier.fillMaxSize().padding(padding), contentPadding = PaddingValues(vertical = 8.dp)) {
        if (events.isEmpty()) {
            item {
                Text(
                    stringResource(R.string.activity_empty), style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(24.dp),
                )
            }
        }
        items(events.asReversed()) { ActivityRow(it.kind, hhmm(it.at), it.text) }
        item {
            Text(
                stringResource(R.string.activity_footnote), style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.outline, modifier = Modifier.padding(horizontal = 16.dp, vertical = 16.dp),
            )
        }
    }
}

@Composable
fun AboutScreen(padding: PaddingValues, version: String, gameData: String, openUrl: (String) -> Unit, openThanks: () -> Unit) = Scrolling(padding) {
    Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Spark(40)
        Spacer(Modifier.height(8.dp))
        Text(stringResource(R.string.app_name), style = MaterialTheme.typography.headlineSmall)
        Text(stringResource(R.string.about_version, version, gameData), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
    Group {
        ListRow(stringResource(R.string.source_code), "tawan475/teyvat.me-capture", R.drawable.ic_github, onClick = { openUrl(Links.SOURCE) }) { External() }
        CardDivider()
        ListRow(Links.trackerHost, stringResource(R.string.teyvat_site_line), R.drawable.ic_globe, onClick = { openUrl(Links.trackerSite) }) { External() }
        CardDivider()
        ListRow(stringResource(R.string.report_bug), stringResource(R.string.report_bug_line), R.drawable.ic_bug, onClick = { openUrl(Links.ISSUES) }) { External() }
        CardDivider()
        ListRow(stringResource(R.string.thanks_and_licences), null, R.drawable.ic_heart, onClick = openThanks) { Chevron() }
    }
    Spacer(Modifier.height(20.dp))
    FanLine(Modifier.padding(horizontal = 16.dp))
}

private val THANKS = listOf(
    "Erik Gilling" to "Irminsul, anime-game-data",
    "PJK136" to "stardb-exporter fork",
    "juliuskreutz" to "stardb-exporter",
    "hashblen" to "auto-artifactarium",
    "IceDynamix" to "HSR network scanning",
    "emmachase" to "pktmon",
    "Genshin Optimizer" to "GOOD format",
    "Inventory Kamera" to "Where it started",
)

@Composable
fun ThanksScreen(padding: PaddingValues, openLicences: () -> Unit) = Scrolling(padding) {
    Spacer(Modifier.height(8.dp))
    Group {
        THANKS.forEachIndexed { i, (who, what) ->
            if (i > 0) CardDivider()
            ListRow(who, what)
        }
        CardDivider()
        ListRow(stringResource(R.string.licences), null, R.drawable.ic_book_open, onClick = openLicences) { Chevron() }
    }
    Text(
        stringResource(R.string.hoyoverse_names), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline,
        textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth().padding(24.dp),
    )
}

private val LICENCES = listOf(
    "Teyvat.me Capture" to "MIT · THIRD_PARTY_NOTICES.md",
    "Irminsul, anime-game-data" to "MIT · Erik Gilling",
    "auto-artifactarium, reliquary" to "MIT · IceDynamix and contributors",
    "Outfit" to "SIL Open Font License 1.1",
    "Lucide icons" to "ISC",
    "ipstack, tokio, jni and other Rust crates" to "MIT or Apache 2.0",
    "AndroidX, Jetpack Compose" to "Apache 2.0",
)

@Composable
fun LicencesScreen(padding: PaddingValues, openUrl: (String) -> Unit, openNotices: () -> Unit) = Scrolling(padding) {
    Spacer(Modifier.height(8.dp))
    Group {
        LICENCES.forEachIndexed { i, (what, licence) ->
            if (i > 0) CardDivider()
            ListRow(what, licence)
        }
        CardDivider()
        ListRow(stringResource(R.string.third_party_notices), "THIRD_PARTY_NOTICES.md · LICENSE", R.drawable.ic_file_json, onClick = openNotices) { Chevron() }
        CardDivider()
        ListRow(stringResource(R.string.source_code), "tawan475/teyvat.me-capture", R.drawable.ic_github, onClick = { openUrl(Links.SOURCE) }) { External() }
    }
}

/** The repo's THIRD_PARTY_NOTICES.md and LICENSE, as the build bundled them. */
@Composable
fun NoticesScreen(padding: PaddingValues) = Scrolling(padding) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val text = remember {
        listOf("THIRD_PARTY_NOTICES.md", "LICENSE").joinToString("\n\n") { name ->
            runCatching { context.assets.open(name).bufferedReader().use { it.readText() } }.getOrDefault("")
        }.trim()
    }
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(16.dp),
    )
}
