// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Text
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import me.teyvat.app.BuildConfig
import me.teyvat.app.Hero
import me.teyvat.app.LiveResult
import me.teyvat.app.Links
import me.teyvat.app.R
import me.teyvat.app.Settings
import me.teyvat.app.Summary
import me.teyvat.app.Today
import me.teyvat.app.TrackerState
import me.teyvat.app.UpdateState
import me.teyvat.app.UploadResult
import me.teyvat.app.Whoami
import me.teyvat.app.WishInfo
import me.teyvat.app.WishSync
import me.teyvat.app.grouped
import me.teyvat.app.hhmm
import me.teyvat.app.line
import me.teyvat.app.title
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale

/** What the home screen's buttons do; MainActivity implements it. */
interface HomeActions {
    fun startCapture()
    fun stopCapture()
    fun openGenshin()
    fun gameInstalled(): Boolean
    fun forceStopGenshin()
    fun openActivity()
    fun openSettings()
    fun openExportSettings()
    fun openAbout()
    fun saveGood()
    fun shareGood()
    fun copyGood()
    fun openExportFolder()
    fun setAutoExport(on: Boolean)
    fun copyAchievements(openUrl: String?)
    fun openUrl(url: String)
    /** Copy the wish URL; with [openUrl], open that importer after. */
    fun copyWishUrl(openUrl: String?)
    fun saveWishes()
    fun shareWishes()
    fun cancelWishes()
    fun setSendWishes(on: Boolean)
    fun trackerSignIn()
    fun trackerCancelSignIn()
    fun setTrackerUpload(on: Boolean)
    fun openTeyvat()
    fun updateInstallNow()
    fun updateAllow()
    fun allowBackground()
    fun dismissCrash()
    fun shareLog()
}

/** The tracker row's inputs: the sign-in and the last upload. */
data class TrackerView(
    val state: TrackerState,
    val last: UploadResult?,
    val uploading: Boolean,
    val uploadOn: Boolean,
    val wishesOn: Boolean = true,
    val liveOn: Boolean = true,
    /** The tracker ended the sign-in. */
    val revoked: Boolean = false,
    /** The captured account's UID, for the upload account check. */
    val capturedUid: Long? = null,
    val key: me.teyvat.app.ImportKey = me.teyvat.app.ImportKey(),
    val api: String = Links.DEFAULT_TRACKER_API,
    /** The last stored live sync batch. */
    val lastLive: LiveResult? = null,
    /** The sign-in page while the browser is open on it. */
    val signInUrl: String? = null,
) {
    /** Signed in, or a verified import key: uploads can go. */
    val canUpload get() = state is TrackerState.SignedIn || key.verified
}

/** Now, ticking once a second, for "just now", Starting's 30 s and Today's clocks. */
@Composable
fun rememberNow(): Long {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(1_000)
            now = System.currentTimeMillis()
        }
    }
    return now
}

/**
 * Home, layout G (spec section 4.1): status strip · Traveler · Today ·
 * Export · Wish history · Tracker row in one column; from 720 dp the
 * desktop's two columns (Traveler | Today, Export | Wish history).
 */
@Composable
fun HomeContent(
    hero: Hero,
    summary: Summary,
    settings: Settings,
    startingSince: Long,
    folderLabel: String,
    tracker: TrackerView,
    update: UpdateState,
    capturing: Boolean,
    backgroundAllowed: Boolean,
    crashed: Boolean,
    actions: HomeActions,
    padding: PaddingValues,
) {
    val now = rememberNow()
    BoxWithConstraints {
        val wide = maxWidth >= 720.dp
        LazyColumn(
            contentPadding = PaddingValues(
                start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp,
                bottom = padding.calculateBottomPadding() + 16.dp,
            ),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (crashed) item { CrashCard(actions) }
            if (update.shown()) item { UpdateBannerFor(update, capturing, actions) }
            item { StatusStrip(hero, summary, startingSince, now, actions) }
            if (!backgroundAllowed) item { BackgroundCard(actions) }
            if (wide) {
                item {
                    SideBySide(
                        { m: Modifier -> TravelerCard(summary, now, m) },
                        { m: Modifier -> TodayCard(summary, now, m) },
                    )
                }
                item {
                    SideBySide(
                        { m: Modifier -> ExportCard(summary, settings, folderLabel, actions, m) },
                        { m: Modifier -> WishCard(summary.wish, tracker, settings, actions, m) },
                    )
                }
            } else {
                item { TravelerCard(summary, now) }
                item { TodayCard(summary, now) }
                item { ExportCard(summary, settings, folderLabel, actions) }
                item { WishCard(summary.wish, tracker, settings, actions) }
            }
            item { TrackerRow(tracker, actions) }
        }
    }
}

/** Two cards side by side, each as tall as the taller one. */
@Composable
private fun SideBySide(left: @Composable (Modifier) -> Unit, right: @Composable (Modifier) -> Unit) {
    Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        left(Modifier.weight(1f).fillMaxHeight())
        right(Modifier.weight(1f).fillMaxHeight())
    }
}

// -- status strip ------------------------------------------------------------

@Composable
private fun StatusStrip(hero: Hero, summary: Summary, startingSince: Long, now: Long, actions: HomeActions) {
    val context = LocalContext.current
    val title = stringResource(hero.title())
    val line = hero.line(context, summary)
    val tm = Tm.colors
    val scheme = MaterialTheme.colorScheme
    val capture = stringResource(R.string.step_vpn)
    val open = stringResource(R.string.home_open_genshin)
    val enter = stringResource(R.string.step_enter_world)
    fun steps(a: Step, b: Step, c: Step) = listOf(capture to a, open to b, enter to c)
    val stop: (() -> Unit)? = if (hero == Hero.CaptureOff) null else actions::stopCapture
    when (hero) {
        Hero.CaptureOff -> StatusHero(stop, R.drawable.ic_shield_off, DiscKind.Neutral, title, line, steps(Step.Pending, Step.Pending, Step.Pending)) {
            TmButton(stringResource(R.string.home_start_capture), actions::startCapture, large = true, icon = R.drawable.ic_power)
        }
        Hero.Starting -> StatusHero(stop, null, DiscKind.Neutral, title, line, steps(Step.Now, Step.Pending, Step.Pending), spinning = true) {
            if (now - startingSince > 30_000) {
                TmButton(stringResource(R.string.home_open_activity), actions::openActivity, kind = ButtonKind.Text)
            }
        }
        Hero.Waiting -> StatusHero(stop, R.drawable.ic_gamepad, DiscKind.Neutral, title, line, steps(Step.Done, Step.Now, Step.Pending)) {
            if (actions.gameInstalled()) {
                TmButton(stringResource(R.string.home_open_genshin), actions::openGenshin, kind = ButtonKind.Tonal, large = true, icon = R.drawable.ic_play)
            }
        }
        Hero.GameDetected, Hero.GameAlreadyOpen ->
            StatusHero(stop, R.drawable.ic_gamepad, DiscKind.Neutral, title, line, steps(Step.Done, Step.Now, Step.Pending))
        Hero.LoginSeen -> StatusHero(stop, R.drawable.ic_log_in, DiscKind.Fill, title, line, steps(Step.Done, Step.Done, Step.Now))
        Hero.Decrypting -> StatusHero(
            stop, R.drawable.ic_circle_check, DiscKind.Success, title, line, steps(Step.Done, Step.Done, Step.Done),
            container = tm.successContainer, border = tm.successContainer,
        )
        Hero.MissedAlreadyRunning, Hero.MissedCaptureStopped -> StatusHero(
            stop, R.drawable.ic_triangle_alert, DiscKind.Error, title, line,
            if (hero == Hero.MissedAlreadyRunning) steps(Step.Done, Step.Failed, Step.Pending) else steps(Step.Failed, Step.Pending, Step.Pending),
            container = scheme.errorContainer, border = tm.errorBorder,
        ) {
            Text(stringResource(R.string.home_reenter), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold)
            TmButton(
                stringResource(R.string.home_force_stop), actions::forceStopGenshin, kind = ButtonKind.Danger, large = true,
                trailingIcon = R.drawable.ic_external_link,
            )
            Text(
                stringResource(R.string.home_or_restart), style = MaterialTheme.typography.bodyMedium,
                color = scheme.onSurfaceVariant, modifier = Modifier.align(Alignment.CenterHorizontally).padding(top = 0.dp),
            )
        }
        Hero.KeyLost -> StatusHero(
            stop, R.drawable.ic_triangle_alert, DiscKind.Error, title, line, steps(Step.Done, Step.Done, Step.Failed),
            container = scheme.errorContainer, border = tm.errorBorder,
        ) {
            Text(stringResource(R.string.home_reenter), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold)
            Text(stringResource(R.string.home_data_kept), style = MaterialTheme.typography.bodyMedium, color = scheme.onSurfaceVariant)
        }
    }
}

// -- Traveler ----------------------------------------------------------------

/** "HH:mm · just now" / "· 5 min ago" / "· 1 h ago". */
@Composable
private fun ago(at: Long, now: Long): String {
    val minutes = ((now - at) / 60_000).coerceAtLeast(0)
    val rel = when {
        minutes < 1 -> stringResource(R.string.just_now)
        minutes < 60 -> stringResource(R.string.minutes_ago, minutes.toInt())
        else -> stringResource(R.string.hours_ago, (minutes / 60).toInt())
    }
    return "${hhmm(at)} · $rel"
}

/** A card's header: title, a muted note beside it (baseline), a muted note at the right. */
@Composable
private fun CardTitle(title: String, note: String?, right: String?, modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Row(Modifier.weight(1f), verticalAlignment = Alignment.Bottom) {
            Text(title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (note != null) {
                Spacer(Modifier.width(6.dp))
                Text(
                    note, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(bottom = 1.dp),
                )
            }
        }
        if (right != null) {
            Spacer(Modifier.width(8.dp))
            Text(right, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline, maxLines = 1)
        }
    }
}

/** "AR 60 · WL 9 · Asia", whatever of it is known. */
@Composable
private fun playerLine(summary: Summary): String? {
    val player = summary.player ?: return null
    val parts = listOfNotNull(
        player.ar?.let { stringResource(R.string.captured_ar_only, it) },
        player.wl?.let { stringResource(R.string.player_wl, it) },
        player.server,
    )
    return parts.takeIf { it.isNotEmpty() }?.joinToString(" · ")
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TravelerCard(summary: Summary, now: Long, modifier: Modifier = Modifier) {
    TmCard(modifier) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            val line = playerLine(summary)
            val uid = summary.player?.uid
            val header = @Composable {
                if (summary.captured && line != null) {
                    // The in-game name isn't captured yet: the game's own "Traveler".
                    CardTitle(stringResource(R.string.traveler), line, summary.dataAt?.let { ago(it, now) })
                } else {
                    CardTitle(
                        stringResource(R.string.captured), null,
                        summary.dataAt?.let { ago(it, now) } ?: stringResource(R.string.captured_nothing),
                    )
                }
            }
            if (uid != null) {
                // The UID is never on screen: only a long-press shows it.
                TooltipBox(
                    positionProvider = TooltipDefaults.rememberPlainTooltipPositionProvider(),
                    tooltip = { PlainTooltip { Text(stringResource(R.string.player_uid, uid.toString())) } },
                    state = rememberTooltipState(),
                ) { header() }
            } else {
                header()
            }
            val c = summary.counts
            fun v(n: Int, at: Long?) = if (at == null) null else grouped(n)
            val tiles = listOf(
                Triple(v(c.characters, summary.updatedCharacters), stringResource(R.string.tile_characters), summary.updatedCharacters),
                Triple(v(c.weapons, summary.updatedItems), stringResource(R.string.tile_weapons), summary.updatedItems),
                Triple(v(c.artifacts, summary.updatedItems), stringResource(R.string.tile_artifacts), summary.updatedItems),
                Triple(v(c.materials, summary.updatedItems), stringResource(R.string.tile_materials), summary.updatedItems),
                Triple(v(c.achievements, summary.updatedAchievements), stringResource(R.string.tile_achievements), summary.updatedAchievements),
                // No class stamp of its own: the count itself flashes the tile.
                Triple(c.anecdotes.takeIf { it > 0 }?.let(::grouped), stringResource(R.string.tile_anecdotes), c.anecdotes.toLong().takeIf { it > 0 }),
            )
            val narrow = LocalDensity.current.fontScale >= 1.3f
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                for (row in tiles.chunked(if (narrow) 2 else 3)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for ((value, label, at) in row) StatTile(value, label, Modifier.weight(1f), flashKey = at)
                    }
                }
            }
        }
    }
}

// -- Today -------------------------------------------------------------------

/** "HH:mm" today, or "Thu HH:mm" another day. */
private fun dayTime(atMs: Long, nowMs: Long, locale: Locale): Pair<String?, String> {
    val a = Calendar.getInstance().apply { timeInMillis = atMs }
    val b = Calendar.getInstance().apply { timeInMillis = nowMs }
    val sameDay = a.get(Calendar.YEAR) == b.get(Calendar.YEAR) && a.get(Calendar.DAY_OF_YEAR) == b.get(Calendar.DAY_OF_YEAR)
    val day = if (sameDay) null else SimpleDateFormat("EEE", locale).format(Date(atMs))
    return day to hhmm(atMs)
}

@Composable
private fun TodayCard(summary: Summary, now: Long, modifier: Modifier = Modifier) {
    val today = summary.today ?: Today()
    val locale = LocalContext.current.resources.configuration.locales[0]
    val nowS = now / 1000
    TmCard(modifier) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            CardTitle(
                stringResource(R.string.today), null,
                summary.tasksAt?.let { stringResource(R.string.today_at_login, hhmm(it)) } ?: stringResource(R.string.captured_nothing),
            )
            @Composable
            fun Resin(m: Modifier) {
                val max = Today.RESIN_MAX
                val current = today.resinAt(nowS) ?: return MiniStat(R.drawable.ic_droplet, stringResource(R.string.resin), null, m)
                val fullAt = today.resinFullAt()
                val line = when {
                    current >= max -> stringResource(R.string.full)
                    fullAt == null -> null
                    else -> {
                        val (day, time) = dayTime(fullAt * 1000, now, locale)
                        if (day == null) stringResource(R.string.full_at, time) else stringResource(R.string.full_on, day, time)
                    }
                }
                MiniStat(
                    R.drawable.ic_droplet, stringResource(R.string.resin), grouped(current), m,
                    total = grouped(max), bar = current / max.toFloat(), line = line, lineOk = current >= max,
                )
            }

            @Composable
            fun Realm(m: Modifier) {
                // Value and bar only: the full time needs the comfort table (spec 4.1.3).
                val realm = today.realm
                MiniStat(
                    R.drawable.ic_house, stringResource(R.string.realm_currency), realm?.let { grouped(it.value) }, m,
                    total = realm?.let { grouped(it.limit) },
                    bar = realm?.takeIf { it.limit > 0 }?.let { it.value / it.limit.toFloat() },
                    line = realm?.takeIf { it.limit > 0 && it.value >= it.limit }?.let { stringResource(R.string.full) },
                    lineOk = true,
                )
            }

            @Composable
            fun Commissions(m: Modifier) {
                val c = today.commissions
                val done = c != null && c.limit > 0 && c.value >= c.limit
                MiniStat(
                    R.drawable.ic_list_checks, stringResource(R.string.commissions), c?.value?.toString(), m,
                    total = c?.limit?.toString(), line = if (done) stringResource(R.string.commissions_done) else null, lineOk = done,
                )
            }

            @Composable
            fun Expeditions(m: Modifier) {
                val list = today.expeditions
                val first = today.firstBack(nowS)
                val (line, ok) = when {
                    list == null -> null to false
                    list.isEmpty() -> stringResource(R.string.expeditions_none) to false
                    first == null -> stringResource(R.string.expeditions_all_back) to true
                    else -> {
                        val (day, time) = dayTime(first * 1000, now, locale)
                        stringResource(R.string.expeditions_first_back, if (day == null) time else "$day $time") to false
                    }
                }
                MiniStat(
                    R.drawable.ic_hourglass, stringResource(R.string.expeditions), list?.let { stringResource(R.string.expeditions_out, it.size) }, m,
                    line = line, lineOk = ok,
                )
            }

            @Composable
            fun WeeklyBosses(m: Modifier) {
                val w = today.weeklyBosses
                MiniStat(
                    R.drawable.ic_swords, stringResource(R.string.weekly_bosses), w?.value?.toString(), m,
                    total = w?.limit?.toString(), line = w?.let { stringResource(R.string.weekly_discounts_left) },
                )
            }

            @Composable
            fun Abyss(m: Modifier) {
                val a = today.abyssStars
                val days = today.abyssDaysLeft(nowS)
                MiniStat(
                    R.drawable.ic_star, stringResource(R.string.spiral_abyss), a?.value?.toString(), m,
                    total = a?.limit?.toString(),
                    line = if (a != null && days != null) pluralStringResource(R.plurals.abyss_resets_in, days, days) else null,
                )
            }

            val cells: List<@Composable (Modifier) -> Unit> = listOf(
                { Resin(it) }, { Realm(it) }, { Commissions(it) }, { Expeditions(it) }, { WeeklyBosses(it) }, { Abyss(it) },
            )
            Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                for (pair in cells.chunked(2)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        for (cell in pair) cell(Modifier.weight(1f))
                    }
                }
            }
        }
    }
}

// -- Export ------------------------------------------------------------------

@Composable
private fun ExportCard(summary: Summary, settings: Settings, folderLabel: String, actions: HomeActions, modifier: Modifier = Modifier) {
    var menu by remember { mutableStateOf(false) }
    val enabled = summary.captured
    TmCard(modifier) {
        CardHeader(stringResource(R.string.export_title)) {
            val hasFolder = settings.autoExport || settings.lastSave != null
            IconButton(onClick = actions::openExportFolder, enabled = hasFolder) {
                TmIcon(
                    R.drawable.ic_folder,
                    stringResource(if (hasFolder) R.string.export_open_folder else R.string.export_no_folder),
                    tint = if (hasFolder) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.outline,
                )
            }
            Box {
                IconButton(onClick = { menu = true }) {
                    TmIcon(R.drawable.ic_more_vertical, stringResource(R.string.export_more))
                }
                TmMenu(menu, onDismiss = { menu = false }) {
                    TmMenuItem(stringResource(R.string.export_copy_achievements), R.drawable.ic_trophy) {
                        menu = false
                        actions.copyAchievements(null)
                    }
                    TmMenuItem(stringResource(R.string.export_stardb), R.drawable.ic_external_link) {
                        menu = false
                        actions.copyAchievements(Links.STARDB_ACHIEVEMENTS)
                    }
                    TmMenuItem(stringResource(R.string.export_seelie), R.drawable.ic_external_link) {
                        menu = false
                        actions.copyAchievements(Links.SEELIE_ACHIEVEMENTS)
                    }
                    CardDivider()
                    TmMenuItem(stringResource(R.string.export_filters), R.drawable.ic_sliders) {
                        menu = false
                        actions.openExportSettings()
                    }
                }
            }
        }
        Row(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            TmButton(
                stringResource(R.string.export_save), actions::saveGood, Modifier.weight(2f),
                enabled = enabled, contentDescription = stringResource(R.string.export_save_description),
            )
            TmButton(stringResource(R.string.export_share), actions::shareGood, Modifier.weight(1f), kind = ButtonKind.Outlined, enabled = enabled)
            TmButton(stringResource(R.string.export_copy), actions::copyGood, Modifier.weight(1f), kind = ButtonKind.Outlined, enabled = enabled)
        }
        Spacer(Modifier.weight(1f, fill = false))
        CardDivider()
        val last = settings.lastAutoExport
        val line = when {
            !settings.autoExport -> stringResource(R.string.export_auto_off)
            last != null -> stringResource(R.string.export_auto_saved, hhmm(last.at), last.name)
            else -> stringResource(R.string.export_auto_on, folderLabel)
        }
        SwitchRow(stringResource(R.string.export_auto), line, settings.autoExport, actions::setAutoExport, icon = R.drawable.ic_folder)
    }
}

// -- Wish history ------------------------------------------------------------

/** The banner's name as the game shows it. */
@Composable
private fun bannerName(banner: Int): String = when (banner) {
    302 -> stringResource(R.string.banner_302)
    500 -> stringResource(R.string.banner_500)
    200 -> stringResource(R.string.banner_200)
    100 -> stringResource(R.string.banner_100)
    else -> stringResource(R.string.banner_301)
}

/** How the Send to teyvat.me row reads now. */
private enum class SyncIcon { None, Spinner, Ok, Warn, Error, Clock }

private data class SyncLine(val text: String, val icon: SyncIcon, val error: Boolean = false)

/** The wish sync's line (the desktop's `SyncStatus::line`). */
@Composable
private fun wishSyncLine(wish: WishInfo, tracker: TrackerView, settings: Settings): SyncLine {
    val sync: WishSync = wish.sync
    if (!tracker.canUpload) return SyncLine(stringResource(R.string.wish_sync_no_account, Links.trackerHost), SyncIcon.None)
    if (!settings.sendWishes) return SyncLine(stringResource(R.string.wish_sync_off), SyncIcon.None)
    if (!settings.trackerUpload) return SyncLine(stringResource(R.string.tracker_upload_off), SyncIcon.None)
    if (sync.phase == "syncing") {
        return SyncLine(stringResource(R.string.wish_syncing_banner, bannerName(sync.banner), sync.page), SyncIcon.Spinner)
    }
    val last = sync.last
    if (last != null) {
        val at = hhmm(last.at)
        return when (last.outcome) {
            "sent" -> SyncLine(stringResource(R.string.wish_synced_added, at, grouped(last.added.toInt())), SyncIcon.Ok)
            "upToDate" -> SyncLine(stringResource(R.string.wish_synced_up_to_date, at), SyncIcon.Ok)
            "unfinished" -> SyncLine(stringResource(R.string.wish_synced_more, at), SyncIcon.Ok)
            "keyExpired" -> SyncLine(stringResource(R.string.wish_sync_key_expired, at), SyncIcon.Warn, true)
            "otherAccount" -> SyncLine(stringResource(R.string.wish_sync_failed, at, stringResource(R.string.wish_other_account)), SyncIcon.Error, true)
            else -> SyncLine(stringResource(R.string.wish_sync_failed, at, last.error.orEmpty()), SyncIcon.Error, true)
        }
    }
    return when {
        wish.key != "ready" -> SyncLine(stringResource(R.string.wish_waits_key), SyncIcon.None)
        !wish.uidKnown -> SyncLine(stringResource(R.string.wish_waiting_uid), SyncIcon.Clock)
        else -> SyncLine(stringResource(R.string.wish_send_line), SyncIcon.None)
    }
}

/**
 * Wish history (spec section 4.1.5), the Export card's shape: Copy URL,
 * paimon.moe, StarDB, then the Send to teyvat.me switch. The buttons wait
 * for the game's key (Wish → History in game).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun WishCard(wish: WishInfo, tracker: TrackerView, settings: Settings, actions: HomeActions, modifier: Modifier = Modifier) {
    val ready = wish.key == "ready"
    val read = wish.read
    val running = read.state == "running"
    var menu by remember { mutableStateOf(false) }
    val scheme = MaterialTheme.colorScheme
    TmCard(modifier) {
        CardHeader(
            stringResource(R.string.wish_title),
            if (ready) wish.keyAt?.let { stringResource(R.string.wish_url_ready, hhmm(it)) } else null,
        ) {
            // The UIGF file of the whole history (kept from the earlier card).
            Box {
                IconButton(onClick = { menu = true }, enabled = ready || running) {
                    TmIcon(
                        R.drawable.ic_more_vertical, stringResource(R.string.more),
                        tint = if (ready || running) scheme.onSurfaceVariant else scheme.outline,
                    )
                }
                TmMenu(menu, onDismiss = { menu = false }) {
                    if (running) {
                        TmMenuItem(stringResource(R.string.cancel), R.drawable.ic_x) {
                            menu = false
                            actions.cancelWishes()
                        }
                    } else {
                        TmMenuItem(stringResource(R.string.wish_save), R.drawable.ic_download) {
                            menu = false
                            actions.saveWishes()
                        }
                        TmMenuItem(stringResource(R.string.wish_share), R.drawable.ic_share) {
                            menu = false
                            actions.shareWishes()
                        }
                    }
                }
            }
        }
        val note = when {
            wish.key == "none" -> stringResource(R.string.wish_open_in_game) to false
            wish.key == "expired" -> stringResource(R.string.wish_expired) to false
            running -> stringResource(R.string.wish_reading, bannerName(read.banner), read.page, grouped(read.records)) to false
            read.state == "failed" -> stringResource(R.string.wish_read_failed, read.error.orEmpty()) to true
            else -> null
        }
        if (note != null) {
            Row(Modifier.padding(start = 16.dp, end = 16.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                if (running) {
                    Spinner(size = 14.dp)
                    Spacer(Modifier.width(6.dp))
                }
                Text(
                    note.first, style = MaterialTheme.typography.bodyMedium,
                    color = if (note.second) scheme.error else scheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis,
                )
            }
        }
        FlowRow(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 12.dp, bottom = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            TmButton(
                stringResource(R.string.wish_copy_url), { actions.copyWishUrl(null) },
                kind = ButtonKind.Outlined, icon = R.drawable.ic_copy, enabled = ready,
            )
            TmButton("paimon.moe", { actions.copyWishUrl(Links.PAIMON_WISH_IMPORT) }, kind = ButtonKind.Text, enabled = ready)
            TmButton("StarDB", { actions.copyWishUrl(Links.STARDB_WISH_IMPORT) }, kind = ButtonKind.Text, enabled = ready)
        }
        Spacer(Modifier.weight(1f, fill = false))
        CardDivider()
        val sync = wishSyncLine(wish, tracker, settings)
        val on = tracker.canUpload && settings.sendWishes
        SwitchRow(
            stringResource(R.string.wish_send_to, Links.trackerHost), sync.text, on, actions::setSendWishes,
            enabled = tracker.canUpload,
            lineColor = if (sync.error) scheme.error else Color.Unspecified,
            leading = when (sync.icon) {
                SyncIcon.None -> null
                SyncIcon.Spinner -> ({ Spinner(size = 20.dp) })
                SyncIcon.Ok -> ({ TmIcon(R.drawable.ic_circle_check, null, tint = Tm.colors.success, size = 20.dp) })
                SyncIcon.Warn -> ({ TmIcon(R.drawable.ic_triangle_alert, null, tint = scheme.error, size = 20.dp) })
                SyncIcon.Error -> ({ TmIcon(R.drawable.ic_circle_x, null, tint = scheme.error, size = 20.dp) })
                SyncIcon.Clock -> ({ TmIcon(R.drawable.ic_clock, null, size = 20.dp) })
            },
        )
    }
}

// -- Tracker row -------------------------------------------------------------

/** Where this sign-in's uploads go, as one line (the desktop's `target_label`). */
@Composable
fun targetLine(who: Whoami): String = when {
    who.scope != "account" -> stringResource(R.string.tracker_target_uid)
    // The chosen account was deleted on the tracker: nothing uploads until one is picked again.
    targetMissing(who) -> stringResource(R.string.tracker_target_missing, stringResource(R.string.tracker_configure))
    who.accountName != null && who.uid != null ->
        stringResource(R.string.tracker_target_account, stringResource(R.string.target_name_uid, who.accountName, who.uid))
    who.accountName != null -> stringResource(R.string.tracker_target_account, who.accountName)
    else -> stringResource(R.string.tracker_target_account, stringResource(R.string.target_uid, who.uid.orEmpty()))
}

fun targetMissing(who: Whoami) = who.scope == "account" && who.accountId == null && who.accountName == null

/** The warning when uploads go to one account and the capture is another UID's, or null. */
@Composable
fun uidMismatch(tracker: TrackerView): String? {
    val captured = tracker.capturedUid ?: return null
    // Signed in to one account, or an import key's account (the desktop's check).
    val (name, uid) = when (val state = tracker.state) {
        is TrackerState.SignedIn -> {
            if (state.who.scope != "account") return null
            (state.who.accountName ?: state.who.uid) to state.who.uid
        }
        else -> tracker.key.account?.takeIf { tracker.key.verified }?.let { it.name to it.uid } ?: return null
    }
    val linked = uid?.trim()?.toLongOrNull() ?: return null
    if (linked == captured) return null
    return stringResource(R.string.tracker_uid_mismatch, captured.toString(), name ?: uid, uid)
}

/** The last result: a live batch newer than the last snapshot, or the snapshot's line. */
@Composable
fun resultLine(tracker: TrackerView): Pair<String, Boolean>? {
    val live = tracker.lastLive
    val last = tracker.last
    if (live != null && (last == null || live.at >= last.at)) {
        return stringResource(R.string.tracker_live_line, pluralStringResource(R.plurals.changes, live.changes, grouped(live.changes)), hhmm(live.at)) to false
    }
    return last?.let { it.text to !it.ok }
}

/** The Tracker row's line, and whether it tells of a problem. */
@Composable
private fun trackerLine(tracker: TrackerView): Pair<String, Boolean> {
    val mismatch = uidMismatch(tracker)
    return when (val state = tracker.state) {
        TrackerState.SigningIn -> stringResource(R.string.tracker_waiting_browser) to false
        TrackerState.SignedOut -> when {
            mismatch != null && !tracker.uploading -> mismatch to true
            tracker.revoked -> stringResource(R.string.tracker_revoked) to true
            tracker.key.verified -> when {
                tracker.uploading -> stringResource(R.string.tracker_uploading) to false
                !tracker.uploadOn -> stringResource(R.string.tracker_upload_off) to false
                else -> resultLine(tracker) ?: (stringResource(R.string.key_valid, tracker.key.account!!.name, tracker.key.account.uid) to false)
            }
            else -> stringResource(R.string.tracker_row_line) to false
        }
        is TrackerState.SignedIn -> when {
            mismatch != null && !tracker.uploading -> mismatch to true
            targetMissing(state.who) -> stringResource(R.string.tracker_uploads_not_chosen) to true
            tracker.uploading -> stringResource(R.string.tracker_uploading) to false
            !tracker.uploadOn -> stringResource(R.string.tracker_upload_off) to false
            else -> resultLine(tracker) ?: (targetLine(state.who) to false)
        }
    }
}

/**
 * teyvat.me, one slim row at the foot (spec 4.1.6): Connect; when
 * connected the name opens the dashboard, the switch is auto-upload and
 * the gear opens its page.
 */
@Composable
private fun TrackerRow(tracker: TrackerView, actions: HomeActions) {
    val state = tracker.state
    val scheme = MaterialTheme.colorScheme
    val connected = state is TrackerState.SignedIn || tracker.key.verified
    TmCard {
        Row(
            Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 12.dp, end = 4.dp, top = 6.dp, bottom = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            when {
                state == TrackerState.SigningIn -> Avatar(spinning = true)
                state is TrackerState.SignedIn -> Avatar(letter = state.who.user.take(1).uppercase().ifEmpty { "?" })
                tracker.key.verified -> Avatar(icon = R.drawable.ic_key)
                else -> Avatar(icon = R.drawable.ic_cloud_upload)
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                val link = when (state) {
                    is TrackerState.SignedIn -> state.who.dashboardUrl.takeIf { it.startsWith("http") } ?: Links.trackerSite
                    else -> Links.trackerSite
                }
                Row(
                    Modifier.clickable(enabled = state != TrackerState.SigningIn, role = Role.Button) { actions.openUrl(link) },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(Links.trackerHost, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    if (state != TrackerState.SigningIn) {
                        Spacer(Modifier.width(4.dp))
                        TmIcon(R.drawable.ic_external_link, null, size = 16.dp)
                    }
                }
                val (line, error) = trackerLine(tracker)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (tracker.uploading) {
                        Spinner(size = 14.dp)
                        Spacer(Modifier.width(6.dp))
                    }
                    Text(
                        line, style = MaterialTheme.typography.bodyMedium,
                        color = if (error) scheme.error else scheme.onSurfaceVariant,
                        maxLines = if (line.length > 80) 5 else 2, overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            Spacer(Modifier.width(8.dp))
            when {
                state == TrackerState.SigningIn ->
                    TmButton(stringResource(R.string.cancel), actions::trackerCancelSignIn, kind = ButtonKind.Text, modifier = Modifier.padding(end = 4.dp))
                connected -> {
                    val upload = stringResource(R.string.tracker_upload_to, Links.trackerHost)
                    Box(Modifier.semantics(mergeDescendants = true) { contentDescription = upload }) {
                        TmSwitch(tracker.uploadOn, actions::setTrackerUpload)
                    }
                    IconButton(onClick = actions::openTeyvat) {
                        TmIcon(R.drawable.ic_settings, stringResource(R.string.tracker_settings_of, Links.trackerHost))
                    }
                }
                else -> TmButton(
                    stringResource(R.string.tracker_connect), actions::trackerSignIn, kind = ButtonKind.Tonal,
                    modifier = Modifier.padding(end = 8.dp),
                )
            }
        }
    }
}

// -- banners -----------------------------------------------------------------

/** The app crashed last time (files/crash.txt): Share log, or Dismiss. */
@Composable
private fun CrashCard(actions: HomeActions) {
    TmCard {
        Row(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 12.dp, bottom = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TmIcon(R.drawable.ic_triangle_alert, null, tint = MaterialTheme.colorScheme.error)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.crash_title), style = MaterialTheme.typography.titleSmall)
                Text(
                    stringResource(R.string.crash_line), style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Row(
            Modifier.fillMaxWidth().padding(start = 14.dp, end = 14.dp, bottom = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
        ) {
            TmButton(stringResource(R.string.dismiss), actions::dismissCrash, kind = ButtonKind.Text)
            TmButton(stringResource(R.string.share_log), actions::shareLog, kind = ButtonKind.Tonal, icon = R.drawable.ic_share)
        }
    }
}

/** Battery saving may pause exports and uploads: one tap to allow running in the background. */
@Composable
private fun BackgroundCard(actions: HomeActions) {
    TmCard {
        Row(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 14.dp, top = 12.dp, bottom = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TmIcon(R.drawable.ic_triangle_alert, null, tint = MaterialTheme.colorScheme.error)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.battery_title), style = MaterialTheme.typography.titleSmall)
                Text(
                    stringResource(R.string.battery_off_line), style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.width(8.dp))
            TmButton(stringResource(R.string.allow), actions::allowBackground, kind = ButtonKind.Tonal)
        }
    }
}

fun UpdateState.shown() = when (this) {
    is UpdateState.Downloading, is UpdateState.Ready, is UpdateState.Installing, is UpdateState.NeedsPermission -> true
    is UpdateState.Failed -> version != null
    else -> false
}

/**
 * A new release on its way in (desktop section 2: a banner above the strip):
 * "Update <v>" / "You have <current>", and the one thing to do about it.
 */
@Composable
fun UpdateBannerFor(update: UpdateState, capturing: Boolean, actions: HomeActions, modifier: Modifier = Modifier, details: Boolean = true) {
    val current = stringResource(R.string.update_you_have, BuildConfig.VERSION_NAME)
    val detailsLabel = if (details) stringResource(R.string.details) else null
    when (update) {
        is UpdateState.Downloading -> UpdateBanner(
            stringResource(R.string.update_banner_title, update.version),
            stringResource(R.string.update_downloading, update.version, update.percent), modifier,
            busy = true, action = detailsLabel, onAction = actions::openAbout,
        )
        is UpdateState.Ready -> UpdateBanner(
            stringResource(R.string.update_banner_title, update.version),
            if (update.silent) {
                stringResource(if (capturing) R.string.update_waits_capture else R.string.update_waits_app)
            } else {
                current
            },
            modifier, action = stringResource(R.string.update_install_now), onAction = actions::updateInstallNow,
        )
        is UpdateState.Installing -> UpdateBanner(
            stringResource(R.string.update_installing, update.version), current, modifier, busy = true,
        )
        is UpdateState.NeedsPermission -> UpdateBanner(
            stringResource(R.string.update_banner_title, update.version), stringResource(R.string.update_allow_line), modifier,
            action = stringResource(R.string.update_allow), onAction = actions::updateAllow,
        )
        is UpdateState.Failed -> UpdateBanner(
            stringResource(R.string.update_failed, update.error), current, modifier, error = true,
            action = stringResource(R.string.try_again), onAction = actions::updateInstallNow,
        )
        else -> {}
    }
}
