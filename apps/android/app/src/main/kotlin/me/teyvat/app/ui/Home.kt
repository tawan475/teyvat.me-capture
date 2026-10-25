// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import me.teyvat.app.Hero
import me.teyvat.app.Links
import me.teyvat.app.R
import me.teyvat.app.Settings
import me.teyvat.app.Summary
import me.teyvat.app.WishInfo
import me.teyvat.app.grouped
import me.teyvat.app.hhmm
import me.teyvat.app.line
import me.teyvat.app.title

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
}

/** Now, ticking once a second, for "just now" and Starting's 30 s. */
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

@Composable
fun HomeContent(
    hero: Hero,
    summary: Summary,
    settings: Settings,
    startingSince: Long,
    folderLabel: String,
    actions: HomeActions,
    padding: PaddingValues,
) {
    val now = rememberNow()
    LazyColumn(
        contentPadding = PaddingValues(
            start = 16.dp, end = 16.dp, top = padding.calculateTopPadding(), bottom = padding.calculateBottomPadding() + 16.dp,
        ),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        item { HeroCard(hero, summary, startingSince, now, actions) }
        item { CapturedCard(summary, now) }
        item { WishCard(summary.wish, actions) }
        item { ExportCard(summary, settings, folderLabel, actions) }
        item { DestinationsCard(actions) }
    }
}

@Composable
private fun HeroCard(hero: Hero, summary: Summary, startingSince: Long, now: Long, actions: HomeActions) {
    val context = LocalContext.current
    val title = stringResource(hero.title())
    val line = hero.line(context, summary)
    val tm = Tm.colors
    val scheme = MaterialTheme.colorScheme
    val vpn = stringResource(R.string.step_vpn)
    val login = stringResource(R.string.step_login)
    val key = stringResource(R.string.step_key)
    fun steps(a: Step, b: Step, c: Step) = listOf(vpn to a, login to b, key to c)
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
        Hero.Waiting, Hero.AtTitleScreen -> StatusHero(stop, R.drawable.ic_gamepad, DiscKind.Neutral, title, line, steps(Step.Done, Step.Now, Step.Pending)) {
            if (hero == Hero.Waiting) {
                InfoChip(stringResource(R.string.home_open_before))
                if (actions.gameInstalled()) {
                    TmButton(stringResource(R.string.home_open_genshin), actions::openGenshin, kind = ButtonKind.Tonal, large = true, icon = R.drawable.ic_play)
                }
            }
        }
        Hero.LoginSeen -> StatusHero(stop, R.drawable.ic_log_in, DiscKind.Fill, title, line, steps(Step.Done, Step.Done, Step.Now))
        Hero.Decrypting -> StatusHero(stop, 
            R.drawable.ic_circle_check, DiscKind.Success, title, line, steps(Step.Done, Step.Done, Step.Done),
            container = tm.successContainer, border = tm.successContainer,
        )
        Hero.MissedAlreadyRunning, Hero.MissedCaptureStopped -> StatusHero(stop, 
            R.drawable.ic_triangle_alert, DiscKind.Error, title, line,
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
                color = scheme.onSurfaceVariant, modifier = Modifier.align(Alignment.CenterHorizontally),
            )
        }
        Hero.KeyLost -> StatusHero(stop, 
            R.drawable.ic_triangle_alert, DiscKind.Error, title, line, steps(Step.Done, Step.Done, Step.Failed),
            container = scheme.errorContainer, border = tm.errorBorder,
        ) {
            Text(stringResource(R.string.home_reenter), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold)
            Text(stringResource(R.string.home_data_kept), style = MaterialTheme.typography.bodyMedium, color = scheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun InfoChip(text: String) {
    Row(
        Modifier
            .clip(RoundedCornerShape(8.dp))
            .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(8.dp))
            .padding(horizontal = 10.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TmIcon(R.drawable.ic_info, null, size = 16.dp)
        Spacer(Modifier.width(6.dp))
        Text(text, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

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

@Composable
private fun CapturedCard(summary: Summary, now: Long) {
    TmCard {
        Row(Modifier.fillMaxWidth().padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.captured), style = MaterialTheme.typography.titleSmall)
                val player = summary.player
                val sub = when {
                    !summary.captured -> stringResource(R.string.captured_nothing)
                    player?.ar != null && player.server != null -> stringResource(R.string.captured_ar, player.ar, player.server)
                    player?.ar != null -> stringResource(R.string.captured_ar_only, player.ar)
                    else -> null
                }
                if (sub != null) {
                    Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            summary.dataAt?.let {
                Text(ago(it, now), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline)
            }
        }
        if (summary.captured) {
            val c = summary.counts
            fun v(n: Int, at: Long?) = if (at == null) null else grouped(n)
            val narrow = LocalDensity.current.fontScale >= 1.3f
            val tiles = listOf(
                Triple(v(c.characters, summary.updatedCharacters), stringResource(R.string.tile_characters), summary.updatedCharacters),
                Triple(v(c.weapons, summary.updatedItems), stringResource(R.string.tile_weapons), summary.updatedItems),
                Triple(v(c.artifacts, summary.updatedItems), stringResource(R.string.tile_artifacts), summary.updatedItems),
                Triple(v(c.materials, summary.updatedItems), stringResource(R.string.tile_materials), summary.updatedItems),
                Triple(v(c.achievements, summary.updatedAchievements), stringResource(R.string.tile_achievements), summary.updatedAchievements),
            )
            val rows = if (narrow) tiles.chunked(2) else listOf(tiles.take(3), tiles.drop(3))
            Column(Modifier.padding(start = 14.dp, end = 14.dp, bottom = 14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                for (row in rows) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for ((value, label, at) in row) {
                            StatTile(value, label, Modifier.weight(1f), flashKey = at)
                        }
                        if (narrow && row.size == 1) Spacer(Modifier.weight(1f))
                    }
                }
            }
        }
    }
}

@Composable
private fun ExportCard(summary: Summary, settings: Settings, folderLabel: String, actions: HomeActions) {
    var menu by remember { mutableStateOf(false) }
    val enabled = summary.captured
    TmCard {
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
            Modifier.fillMaxWidth().padding(start = 14.dp, end = 14.dp, bottom = 14.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            val described = stringResource(R.string.export_save_description)
            TmButton(
                stringResource(R.string.export_save), actions::saveGood, Modifier.weight(1.7f),
                enabled = enabled, contentDescription = described,
            )
            TmButton(stringResource(R.string.export_share), actions::shareGood, Modifier.weight(1f), kind = ButtonKind.Outlined, enabled = enabled)
            TmButton(stringResource(R.string.export_copy), actions::copyGood, Modifier.weight(1f), kind = ButtonKind.Outlined, enabled = enabled)
        }
        CardDivider()
        val last = settings.lastAutoExport
        val line = when {
            !settings.autoExport -> stringResource(R.string.export_auto_off)
            last != null -> stringResource(R.string.export_auto_saved, hhmm(last.at), last.name)
            else -> stringResource(R.string.export_auto_on, folderLabel)
        }
        SwitchRow(
            stringResource(R.string.export_auto), line, settings.autoExport, actions::setAutoExport,
            icon = R.drawable.ic_folder,
        )
    }
}

@Composable
private fun DestinationsCard(actions: HomeActions) {
    TmCard {
        CardHeader(stringResource(R.string.destinations), stringResource(R.string.optional))
        Row(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 14.dp, bottom = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Avatar(icon = R.drawable.ic_cloud_upload)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Row(
                    Modifier.clickable { actions.openUrl(Links.trackerSite) }.semantics { contentDescription = Links.trackerHost },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        Links.trackerHost, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.primary,
                        maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false),
                    )
                    Spacer(Modifier.width(4.dp))
                    TmIcon(R.drawable.ic_external_link, null, tint = MaterialTheme.colorScheme.primary, size = 16.dp)
                }
                Text(
                    stringResource(R.string.not_in_this_version),
                    style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2, overflow = TextOverflow.Ellipsis,
                )
            }
            Spacer(Modifier.width(8.dp))
            TmButton(stringResource(R.string.connect), {}, kind = ButtonKind.Tonal, enabled = false)
        }
    }
}

/** The banner's name as the game shows it. */
@Composable
private fun bannerName(banner: Int): String = when (banner) {
    302 -> stringResource(R.string.banner_302)
    500 -> stringResource(R.string.banner_500)
    200 -> stringResource(R.string.banner_200)
    100 -> stringResource(R.string.banner_100)
    else -> stringResource(R.string.banner_301)
}

/**
 * Wish history (spec section 4.1.3): greyed until the game sends its key
 * (Wish → History in game); then Copy URL for paimon.moe or StarDB, and Save
 * or Share of the whole history as UIGF.
 */
@Composable
private fun WishCard(wish: WishInfo, actions: HomeActions) {
    when (wish.key) {
        "none" -> return GreyCard(R.drawable.ic_sparkles, stringResource(R.string.wish_title), stringResource(R.string.wish_open_in_game))
        "expired" -> return GreyCard(R.drawable.ic_sparkles, stringResource(R.string.wish_title), stringResource(R.string.wish_expired))
    }
    var menu by remember { mutableStateOf(false) }
    val read = wish.read
    val running = read.state == "running"
    TmCard {
        Row(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TmIcon(R.drawable.ic_sparkles, null, tint = MaterialTheme.colorScheme.primary)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f).padding(vertical = 8.dp)) {
                Text(stringResource(R.string.wish_title), style = MaterialTheme.typography.titleSmall)
                val (line, error) = when (read.state) {
                    "running" -> stringResource(R.string.wish_reading, bannerName(read.banner), read.page, grouped(read.records)) to false
                    "done" -> stringResource(R.string.wish_read_done, hhmm(read.at), grouped(read.records)) to false
                    "failed" -> stringResource(R.string.wish_read_failed, read.error.orEmpty()) to true
                    else -> (if (wish.uidKnown) stringResource(R.string.wish_ready) else stringResource(R.string.wish_waiting_uid)) to false
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (running) {
                        Spinner(size = 14.dp)
                        Spacer(Modifier.width(6.dp))
                    }
                    Text(
                        line, style = MaterialTheme.typography.bodyMedium,
                        color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 2, overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            Box {
                IconButton(onClick = { menu = true }) {
                    TmIcon(R.drawable.ic_more_vertical, stringResource(R.string.more))
                }
                TmMenu(menu, onDismiss = { menu = false }) {
                    TmMenuItem(stringResource(R.string.wish_open_paimon), R.drawable.ic_external_link) {
                        menu = false
                        actions.copyWishUrl(Links.PAIMON_WISH_IMPORT)
                    }
                    TmMenuItem(stringResource(R.string.wish_open_stardb), R.drawable.ic_external_link) {
                        menu = false
                        actions.copyWishUrl(Links.STARDB_WISH_IMPORT)
                    }
                    CardDivider()
                    TmMenuItem(stringResource(R.string.wish_share), R.drawable.ic_share) {
                        menu = false
                        actions.shareWishes()
                    }
                }
            }
        }
        Row(
            Modifier.fillMaxWidth().padding(start = 14.dp, end = 14.dp, bottom = 14.dp, top = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            TmButton(
                stringResource(R.string.wish_copy_url), { actions.copyWishUrl(null) }, Modifier.weight(1f),
                kind = ButtonKind.Outlined, icon = R.drawable.ic_copy,
            )
            if (running) {
                TmButton(stringResource(R.string.cancel), actions::cancelWishes, Modifier.weight(1f), kind = ButtonKind.Text)
            } else {
                TmButton(
                    stringResource(R.string.wish_save), actions::saveWishes, Modifier.weight(1f),
                    kind = ButtonKind.Outlined, icon = R.drawable.ic_download,
                    contentDescription = stringResource(R.string.wish_save_description),
                )
            }
        }
    }
}
