// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app.ui

import androidx.compose.foundation.clickable
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
    fun allowBackground()
    fun openUrl(url: String)
    fun setLanguage(tag: String?)
}

@Composable
fun SettingsScreen(
    padding: PaddingValues,
    settings: Settings,
    tracker: TrackerView,
    update: UpdateState,
    backgroundAllowed: Boolean,
    actions: SettingsActions,
) = Scrolling(padding) {
    SectionLabel(stringResource(R.string.section_app))
    Group {
        ListRow(stringResource(R.string.always_on_vpn), stringResource(R.string.always_on_vpn_line), R.drawable.ic_power, onClick = actions::openAlwaysOnVpn) { External() }
        CardDivider()
        ListRow(stringResource(R.string.notifications), stringResource(R.string.notifications_line), R.drawable.ic_bell, onClick = actions::openNotificationSettings) { External() }
        CardDivider()
        ListRow(
            stringResource(R.string.battery_title),
            stringResource(if (backgroundAllowed) R.string.battery_on_line else R.string.battery_off_line),
            R.drawable.ic_power, onClick = actions::allowBackground,
        ) { External() }
        CardDivider()
        ListRow(
            stringResource(R.string.battery_more), stringResource(R.string.battery_more_line), R.drawable.ic_info,
            onClick = { actions.openUrl(me.teyvat.app.Battery.DONT_KILL_MY_APP) },
        ) { External() }
        CardDivider()
        LanguageRow(actions)
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
        CardDivider()
        ListRow(
            stringResource(R.string.release_notes), me.teyvat.app.BuildConfig.VERSION_NAME, R.drawable.ic_info,
            onClick = { actions.openUrl("${Links.SOURCE}/releases/tag/v${me.teyvat.app.BuildConfig.VERSION_NAME}") },
        ) { External() }
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

/** The app's language: the system's, or one the game ships ([me.teyvat.app.AppLocale]). */
@Composable
private fun LanguageRow(actions: SettingsActions) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val current = remember { me.teyvat.app.AppLocale.current(context) }
    var picking by remember { mutableStateOf(false) }
    val system = stringResource(R.string.language_system)
    val name = me.teyvat.app.AppLocale.LANGUAGES.firstOrNull { it.first == current }?.second ?: system
    ListRow(stringResource(R.string.language), name, R.drawable.ic_globe, onClick = { picking = true }) { Chevron() }
    if (picking) {
        androidx.compose.material3.AlertDialog(
            onDismissRequest = { picking = false },
            confirmButton = {},
            dismissButton = {
                androidx.compose.material3.TextButton(onClick = { picking = false }) { Text(stringResource(R.string.cancel)) }
            },
            title = { Text(stringResource(R.string.language)) },
            text = {
                androidx.compose.foundation.lazy.LazyColumn {
                    val options = listOf<Pair<String?, String>>(null to system) + me.teyvat.app.AppLocale.LANGUAGES
                    items(options.size) { i ->
                        val (tag, label) = options[i]
                        Row(
                            Modifier.fillMaxWidth().clickable {
                                picking = false
                                if (tag != current) actions.setLanguage(tag)
                            }.padding(vertical = 10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            androidx.compose.material3.RadioButton(selected = tag == current, onClick = null)
                            Spacer(Modifier.width(12.dp))
                            Text(label, style = MaterialTheme.typography.bodyLarge)
                        }
                    }
                }
            },
        )
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
    fun setSendWishes(on: Boolean)
    fun setLiveSync(on: Boolean)
    fun trackerSignOut()
    fun openUrl(url: String)
    fun setImportKey(key: String)
    fun clearImportKey()
    fun setTrackerApi(api: String)
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
                is TrackerState.SignedIn -> {}
            }
            if (tracker.canUpload) {
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
        if (tracker.canUpload) {
            CardDivider()
            SwitchRow(
                stringResource(R.string.tracker_upload_switch), stringResource(R.string.tracker_upload_switch_line), tracker.uploadOn,
                actions::setTrackerUpload, icon = R.drawable.ic_cloud_upload,
            )
            CardDivider()
            SwitchRow(
                stringResource(R.string.live_sync), stringResource(R.string.live_sync_line), tracker.liveOn && tracker.uploadOn,
                actions::setLiveSync, icon = R.drawable.ic_refresh, enabled = tracker.uploadOn,
            )
            CardDivider()
            SwitchRow(
                stringResource(R.string.wish_send, Links.trackerHost), stringResource(R.string.wish_send_line), tracker.wishesOn,
                actions::setSendWishes, icon = R.drawable.ic_sparkles,
            )
        }
        if (state is TrackerState.SignedIn) {
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
    AdvancedTracker(tracker, actions)
}

/** The desktop's Advanced: the tracker address, and the import key used while signed out. */
@Composable
private fun AdvancedTracker(tracker: TrackerView, actions: TeyvatActions) {
    SectionLabel(stringResource(R.string.advanced))
    Group {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.tracker_address), style = MaterialTheme.typography.bodyLarge)
            Text(
                stringResource(R.string.tracker_address_hint), style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            var address by remember(tracker.api) { mutableStateOf(tracker.api) }
            var confirm by remember { mutableStateOf<String?>(null) }
            val wanted = address.trim().trimEnd('/')
            val valid = (wanted.startsWith("https://") || wanted.startsWith("http://")) &&
                !android.net.Uri.parse(wanted).host.isNullOrEmpty()
            androidx.compose.material3.OutlinedTextField(
                address, { address = it }, Modifier.fillMaxWidth(), singleLine = true,
                placeholder = { Text(Links.DEFAULT_TRACKER_API) },
                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                    keyboardType = androidx.compose.ui.text.input.KeyboardType.Uri,
                ),
            )
            Text(
                if (valid) stringResource(R.string.default_x, Links.DEFAULT_TRACKER_API) else stringResource(R.string.not_http),
                style = MaterialTheme.typography.bodySmall,
                color = if (valid) MaterialTheme.colorScheme.outline else MaterialTheme.colorScheme.error,
            )
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
                TmButton(stringResource(R.string.default_address), { address = Links.DEFAULT_TRACKER_API }, kind = ButtonKind.Text)
                TmButton(stringResource(R.string.apply), { confirm = wanted }, kind = ButtonKind.Tonal, enabled = valid && wanted != tracker.api)
            }
            confirm?.let { to ->
                androidx.compose.material3.AlertDialog(
                    onDismissRequest = { confirm = null },
                    title = { Text(stringResource(R.string.tracker_address)) },
                    text = {
                        Text(
                            (if (tracker.state is TrackerState.SignedIn) stringResource(R.string.signed_out_of, Links.trackerHost) + "\n\n" else "") +
                                stringResource(R.string.uploads_will_go, to),
                        )
                    },
                    confirmButton = {
                        androidx.compose.material3.TextButton(onClick = {
                            confirm = null
                            actions.setTrackerApi(to)
                        }) { Text(stringResource(R.string.apply)) }
                    },
                    dismissButton = {
                        androidx.compose.material3.TextButton(onClick = { confirm = null }) { Text(stringResource(R.string.cancel)) }
                    },
                )
            }
        }
        CardDivider()
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.import_key), style = MaterialTheme.typography.bodyLarge)
            Text(
                stringResource(R.string.import_key_hint), style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            var key by remember(tracker.key.key) { mutableStateOf(tracker.key.key.orEmpty()) }
            androidx.compose.material3.OutlinedTextField(
                key, { key = it }, Modifier.fillMaxWidth(), singleLine = true,
                visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                    keyboardType = androidx.compose.ui.text.input.KeyboardType.Password,
                    imeAction = androidx.compose.ui.text.input.ImeAction.Done,
                ),
                keyboardActions = androidx.compose.foundation.text.KeyboardActions(onDone = { actions.setImportKey(key) }),
            )
            val k = tracker.key
            val (status, ok) = when {
                k.checking -> stringResource(R.string.key_verifying) to null
                k.account != null -> stringResource(R.string.key_valid, k.account.name, k.account.uid) to true
                k.key != null -> stringResource(R.string.key_invalid) to false
                else -> null to null
            }
            status?.let {
                Text(
                    it, style = MaterialTheme.typography.bodySmall,
                    color = when (ok) {
                        true -> Tm.colors.success
                        false -> MaterialTheme.colorScheme.error
                        null -> MaterialTheme.colorScheme.onSurfaceVariant
                    },
                )
            }
            Text(
                stringResource(R.string.key_warning) + " · " + stringResource(R.string.key_signed_out_only),
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline,
            )
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
                if (k.key != null) {
                    TmButton(stringResource(R.string.clear), {
                        key = ""
                        actions.clearImportKey()
                    }, kind = ButtonKind.Text, contentDescription = stringResource(R.string.key_remove))
                }
                TmButton(
                    stringResource(R.string.apply), { actions.setImportKey(key) }, kind = ButtonKind.Tonal,
                    enabled = key.isNotBlank() && !k.checking && (key.trim() != k.key || k.account == null),
                )
            }
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
