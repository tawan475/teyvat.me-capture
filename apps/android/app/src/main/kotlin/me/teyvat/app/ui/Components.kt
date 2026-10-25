// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app.ui

import android.provider.Settings
import androidx.annotation.DrawableRes
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import me.teyvat.app.R

/** A lucide line icon from res/drawable. */
@Composable
fun TmIcon(@DrawableRes icon: Int, description: String?, modifier: Modifier = Modifier, tint: Color = Color.Unspecified, size: Dp = 22.dp) {
    Icon(
        painterResource(icon),
        contentDescription = description,
        modifier = modifier.size(size),
        tint = if (tint == Color.Unspecified) MaterialTheme.colorScheme.onSurfaceVariant else tint,
    )
}

/** Whether the system asks for reduced motion (animations off). */
@Composable
fun reducedMotion(): Boolean {
    val context = LocalContext.current
    return remember {
        Settings.Global.getFloat(context.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f
    }
}

/** loader-circle, rotating unless motion is reduced. */
@Composable
fun Spinner(tint: Color = MaterialTheme.colorScheme.primary, size: Dp = 20.dp, description: String? = null) {
    val still = reducedMotion()
    val angle = if (still) {
        0f
    } else {
        val turn by rememberInfiniteTransition(label = "spin").animateFloat(
            0f, 360f, infiniteRepeatable(tween(900, easing = LinearEasing), RepeatMode.Restart), label = "angle",
        )
        turn
    }
    TmIcon(R.drawable.ic_loader, description, Modifier.rotate(angle), tint, size)
}

/** ✦ Teyvat.me Capture. */
@Composable
fun Brand(starSize: Int = 18, textStyle: androidx.compose.ui.text.TextStyle = MaterialTheme.typography.titleLarge) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Spark(starSize)
        Spacer(Modifier.width(8.dp))
        Text(
            buildAnnotatedString {
                withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(stringResource(R.string.brand)) }
                withStyle(SpanStyle(color = MaterialTheme.colorScheme.onSurfaceVariant, fontWeight = FontWeight.Normal)) {
                    append(" " + stringResource(R.string.brand_suffix))
                }
            },
            style = textStyle,
            color = MaterialTheme.colorScheme.onSurface,
            maxLines = 1,
        )
    }
}

/** The amber ✦ with its glow. */
@Composable
fun Spark(size: Int) {
    Text(
        "✦",
        fontSize = size.sp,
        color = Tm.colors.spark,
        modifier = Modifier.shadow(8.dp, CircleShape, ambientColor = Tm.colors.sparkGlow, spotColor = Tm.colors.sparkGlow),
    )
}

/** 56 dp top bar: the brand (Home) or a back arrow and a title. */
@Composable
fun TmTopBar(title: String? = null, onBack: (() -> Unit)? = null, actions: @Composable RowScope.() -> Unit = {}) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(56.dp)
            .padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (onBack != null) {
            IconButton(onClick = onBack) {
                TmIcon(R.drawable.ic_arrow_left, stringResource(R.string.back), tint = MaterialTheme.colorScheme.onSurface)
            }
        } else {
            Spacer(Modifier.width(12.dp))
        }
        Box(Modifier.weight(1f)) {
            if (title == null) {
                Brand()
            } else {
                Text(title, style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        actions()
    }
}

enum class ButtonKind { Filled, Tonal, Outlined, Text, Danger }

/** The kit's buttons; filled ones stay indigo + white in both themes. */
@Composable
fun TmButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    kind: ButtonKind = ButtonKind.Filled,
    enabled: Boolean = true,
    large: Boolean = false,
    @DrawableRes icon: Int? = null,
    @DrawableRes trailingIcon: Int? = null,
    contentDescription: String? = null,
) {
    val height = if (large) 48.dp else 36.dp
    val m = (if (large) modifier.fillMaxWidth() else modifier).heightIn(min = height)
        .let { if (contentDescription != null) it.semantics { this.contentDescription = contentDescription } else it }
    val padding = PaddingValues(horizontal = if (large) 20.dp else 12.dp)
    val content: @Composable RowScope.() -> Unit = {
        val color = androidx.compose.material3.LocalContentColor.current
        if (icon != null) {
            TmIcon(icon, null, tint = color, size = 16.dp)
            Spacer(Modifier.width(8.dp))
        }
        Text(text, style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (trailingIcon != null) {
            Spacer(Modifier.width(6.dp))
            TmIcon(trailingIcon, null, tint = color, size = 16.dp)
        }
    }
    val tm = Tm.colors
    val scheme = MaterialTheme.colorScheme
    when (kind) {
        ButtonKind.Filled, ButtonKind.Danger, ButtonKind.Tonal -> {
            val (bg, fg) = when (kind) {
                ButtonKind.Filled -> tm.fill to tm.onFill
                ButtonKind.Danger -> tm.danger to tm.onDanger
                else -> scheme.primaryContainer to scheme.onPrimaryContainer
            }
            Button(
                onClick, m, enabled,
                shape = RoundedCornerShape(12.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = bg, contentColor = fg,
                    disabledContainerColor = bg.copy(alpha = 0.38f), disabledContentColor = fg.copy(alpha = 0.6f),
                ),
                contentPadding = padding, content = content,
            )
        }
        ButtonKind.Outlined -> OutlinedButton(
            onClick, m, enabled,
            shape = RoundedCornerShape(12.dp),
            border = BorderStroke(1.dp, if (enabled) tm.outlinedBorder else tm.outlinedBorder.copy(alpha = 0.38f)),
            colors = ButtonDefaults.outlinedButtonColors(contentColor = scheme.primary),
            contentPadding = padding, content = content,
        )
        ButtonKind.Text -> TextButton(
            onClick, m, enabled,
            colors = ButtonDefaults.textButtonColors(contentColor = scheme.primary),
            contentPadding = PaddingValues(horizontal = 12.dp), content = content,
        )
    }
}

/** A card: surfaceContainerLowest, 16 dp radius, 1 dp border, no elevation. */
@Composable
fun TmCard(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.large)
            .background(MaterialTheme.colorScheme.surfaceContainerLowest)
            .border(1.dp, MaterialTheme.colorScheme.outlineVariant, MaterialTheme.shapes.large),
        content = content,
    )
}

/** A section label over a group of rows. */
@Composable
fun SectionLabel(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(start = 16.dp, top = 16.dp, bottom = 6.dp),
    )
}

/** A row: optional icon, title and one line; the row is the click target. */
@Composable
fun ListRow(
    title: String,
    line: String? = null,
    @DrawableRes icon: Int? = null,
    lineColor: Color = Color.Unspecified,
    lineLines: Int = 1,
    enabled: Boolean = true,
    onClick: (() -> Unit)? = null,
    trailing: @Composable RowScope.() -> Unit = {},
) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .let { if (onClick != null) it.clickable(enabled = enabled, onClick = onClick) else it }
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) {
            TmIcon(icon, null, tint = if (enabled) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.outline)
            Spacer(Modifier.width(14.dp))
        }
        Column(Modifier.weight(1f)) {
            Text(
                title,
                style = MaterialTheme.typography.bodyLarge,
                color = if (enabled) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1, overflow = TextOverflow.Ellipsis,
            )
            if (line != null) {
                Text(
                    line,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (lineColor == Color.Unspecified) MaterialTheme.colorScheme.onSurfaceVariant else lineColor,
                    maxLines = lineLines, overflow = TextOverflow.Ellipsis,
                )
            }
        }
        trailing()
    }
}

@Composable
fun TmSwitch(checked: Boolean, onChange: ((Boolean) -> Unit)?, enabled: Boolean = true) {
    Switch(
        checked = checked,
        onCheckedChange = onChange,
        enabled = enabled,
        colors = SwitchDefaults.colors(
            checkedTrackColor = Tm.colors.fill,
            checkedThumbColor = Color.White,
            uncheckedTrackColor = MaterialTheme.colorScheme.surfaceContainerHighest,
            uncheckedBorderColor = MaterialTheme.colorScheme.outline,
            uncheckedThumbColor = MaterialTheme.colorScheme.outline,
        ),
    )
}

/** A switch row: the whole row toggles (role Switch). */
@Composable
fun SwitchRow(
    title: String,
    line: String?,
    checked: Boolean,
    onChange: (Boolean) -> Unit,
    @DrawableRes icon: Int? = null,
    enabled: Boolean = true,
    lineColor: Color = Color.Unspecified,
    leading: (@Composable () -> Unit)? = null,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .toggleable(value = checked, enabled = enabled, role = Role.Switch, onValueChange = onChange)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (leading != null) {
            leading()
            Spacer(Modifier.width(16.dp))
        } else if (icon != null) {
            TmIcon(icon, null)
            Spacer(Modifier.width(16.dp))
        }
        Column(Modifier.weight(1f)) {
            Text(
                title, style = MaterialTheme.typography.bodyLarge,
                color = if (enabled) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1, overflow = TextOverflow.Ellipsis,
            )
            if (line != null) {
                Text(
                    line, style = MaterialTheme.typography.bodyMedium,
                    color = if (lineColor == Color.Unspecified) MaterialTheme.colorScheme.onSurfaceVariant else lineColor,
                    maxLines = 2, overflow = TextOverflow.Ellipsis,
                )
            }
        }
        Spacer(Modifier.width(12.dp))
        TmSwitch(checked, null, enabled)
    }
}

/**
 * The overflow menu: a rounded card with a border and a shadow, like the
 * cards it opens from (Material's plain menu read as unfinished).
 */
@Composable
fun TmMenu(expanded: Boolean, onDismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    DropdownMenu(
        expanded = expanded,
        onDismissRequest = onDismiss,
        shape = MaterialTheme.shapes.medium,
        containerColor = MaterialTheme.colorScheme.surfaceContainerLowest,
        tonalElevation = 0.dp,
        shadowElevation = 8.dp,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        content = content,
    )
}

@Composable
fun TmMenuItem(text: String, @DrawableRes icon: Int, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(text, style = MaterialTheme.typography.bodyLarge) },
        leadingIcon = { TmIcon(icon, null, size = 18.dp) },
        onClick = onClick,
        contentPadding = PaddingValues(horizontal = 16.dp),
    )
}

@Composable
fun CardDivider() = HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)

/** A stepper dot's state. */
enum class Step { Pending, Now, Done, Failed }

@Composable
fun Stepper(steps: List<Pair<String, Step>>) {
    Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
        for ((label, step) in steps) {
            val color = when (step) {
                Step.Pending -> MaterialTheme.colorScheme.outline
                Step.Now -> MaterialTheme.colorScheme.primary
                Step.Done -> Tm.colors.success
                Step.Failed -> MaterialTheme.colorScheme.error
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                when (step) {
                    Step.Done -> TmIcon(R.drawable.ic_check, null, tint = color, size = 14.dp)
                    Step.Failed -> TmIcon(R.drawable.ic_x, null, tint = color, size = 14.dp)
                    Step.Now -> Box(Modifier.size(8.dp).clip(CircleShape).background(color))
                    Step.Pending -> Box(Modifier.size(8.dp).clip(CircleShape).border(1.5.dp, color, CircleShape))
                }
                Spacer(Modifier.width(6.dp))
                Text(label, style = MaterialTheme.typography.labelMedium, color = color)
            }
        }
    }
}

/** The disc beside the hero title. */
enum class DiscKind { Neutral, Fill, Success, Error }

/**
 * Disc + headline + line; stepper; an action slot (spec StatusHero). With
 * [onStop], a Stop button beside the title ends the capture.
 */
@Composable
fun StatusHero(
    onStop: (() -> Unit)?,
    @DrawableRes icon: Int?,
    disc: DiscKind,
    title: String,
    line: String,
    steps: List<Pair<String, Step>>?,
    container: Color = MaterialTheme.colorScheme.surfaceContainerLowest,
    border: Color = MaterialTheme.colorScheme.outlineVariant,
    spinning: Boolean = false,
    actions: @Composable ColumnScope.() -> Unit = {},
) {
    val tm = Tm.colors
    val scheme = MaterialTheme.colorScheme
    val (discBg, discFg) = when (disc) {
        DiscKind.Neutral -> scheme.surfaceContainerHighest to scheme.onSurfaceVariant
        DiscKind.Fill -> tm.fill to tm.onFill
        DiscKind.Success -> tm.success to tm.onSuccess
        DiscKind.Error -> tm.errorStrong to tm.onErrorStrong
    }
    Column(
        Modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.extraLarge)
            .background(container)
            .border(1.dp, border, MaterialTheme.shapes.extraLarge)
            .padding(16.dp)
            .semantics(mergeDescendants = false) { liveRegion = LiveRegionMode.Polite },
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(40.dp).clip(CircleShape).background(discBg), contentAlignment = Alignment.Center) {
                if (spinning) Spinner(discFg, 20.dp) else if (icon != null) TmIcon(icon, null, tint = discFg, size = 20.dp)
            }
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.headlineSmall)
                Text(line, style = MaterialTheme.typography.bodyMedium, color = scheme.onSurfaceVariant)
            }
            if (onStop != null) {
                TmButton(
                    stringResource(R.string.stop), onStop, kind = ButtonKind.Outlined, icon = R.drawable.ic_power,
                    contentDescription = stringResource(R.string.stop_capture),
                )
            }
        }
        if (steps != null) Stepper(steps)
        actions()
    }
}

/** A count tile: value + label; `—` while empty. Flashes when [flashKey] changes. */
@Composable
fun StatTile(value: String?, label: String, modifier: Modifier = Modifier, flashKey: Any? = null) {
    val still = reducedMotion()
    var flashing by remember { mutableStateOf(false) }
    var seen by remember { mutableStateOf(flashKey) }
    LaunchedEffect(flashKey) {
        if (flashKey != null && seen != null && flashKey != seen && !still) {
            flashing = true
            delay(600)
            flashing = false
        }
        seen = flashKey
    }
    val bg by animateColorAsState(
        if (flashing) Tm.colors.successContainer else MaterialTheme.colorScheme.surfaceContainerHighest, label = "flash",
    )
    Column(
        modifier
            .clip(RoundedCornerShape(12.dp))
            .background(bg)
            .padding(horizontal = 10.dp, vertical = 8.dp),
    ) {
        Text(value ?: "—", style = MaterialTheme.typography.titleMedium, maxLines = 1)
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** A greyed slim card (not tappable, announced as disabled). */
@Composable
fun GreyCard(@DrawableRes icon: Int, title: String, line: String) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.large)
            .background(MaterialTheme.colorScheme.surfaceContainerHighest)
            .semantics(mergeDescendants = true) { disabled() }
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TmIcon(icon, null, tint = MaterialTheme.colorScheme.outline)
        Spacer(Modifier.width(14.dp))
        Column {
            Text(title, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(line, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** The 48 dp footer: capture dot, the newest event, chevron-up; the bar opens Activity. */
@Composable
fun HomeFooter(dot: Color, line: String, error: Boolean, onClick: () -> Unit) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainerLowest) {
        Column(Modifier.windowInsetsPadding(WindowInsets.navigationBars)) {
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            Row(
                Modifier
                    .fillMaxWidth()
                    .height(48.dp)
                    .clickable(onClick = onClick, onClickLabel = stringResource(R.string.activity))
                    .padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(8.dp).clip(CircleShape).background(dot))
                Spacer(Modifier.width(12.dp))
                Text(
                    line,
                    style = MaterialTheme.typography.bodySmall,
                    color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                TmIcon(R.drawable.ic_chevron_up, null, size = 20.dp)
            }
        }
    }
}

/** One activity line: icon, time, text. */
@Composable
fun ActivityRow(kind: String, time: String, text: String) {
    val (icon, tint) = when (kind) {
        "ok" -> R.drawable.ic_circle_check to Tm.colors.success
        "error" -> R.drawable.ic_circle_x to MaterialTheme.colorScheme.error
        else -> R.drawable.ic_info to MaterialTheme.colorScheme.onSurfaceVariant
    }
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 44.dp)
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TmIcon(icon, null, tint = tint, size = 18.dp)
        Spacer(Modifier.width(12.dp))
        Text(time, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline, modifier = Modifier.width(42.dp))
        Text(
            text,
            style = MaterialTheme.typography.bodyMedium,
            color = if (kind == "error") MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
            maxLines = 2, overflow = TextOverflow.Ellipsis,
        )
    }
}

/** A round avatar (36 dp) with an icon or a letter. */
@Composable
fun Avatar(@DrawableRes icon: Int? = null, letter: String? = null, spinning: Boolean = false) {
    Box(
        Modifier.size(36.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceContainerHighest),
        contentAlignment = Alignment.Center,
    ) {
        when {
            spinning -> Spinner(size = 20.dp)
            icon != null -> TmIcon(icon, null, size = 20.dp)
            letter != null -> Text(letter, style = MaterialTheme.typography.titleSmall)
        }
    }
}

/** A card header: title, optional muted note, actions on the right. */
@Composable
fun CardHeader(title: String, note: String? = null, actions: @Composable RowScope.() -> Unit = {}) {
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 2.dp).heightIn(min = 44.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = MaterialTheme.typography.titleSmall)
        if (note != null) {
            Spacer(Modifier.width(8.dp))
            Text(note, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline)
        }
        Spacer(Modifier.weight(1f))
        actions()
    }
}
