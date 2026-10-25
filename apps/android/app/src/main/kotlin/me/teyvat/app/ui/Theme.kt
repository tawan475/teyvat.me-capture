// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app.ui

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import me.teyvat.app.R

/** The tracker's design tokens on Material 3 (spec section 1.1). */
private val Light = lightColorScheme(
    background = Color(0xFFF8FAFC),
    surface = Color(0xFFF8FAFC),
    surfaceContainerLowest = Color(0xFFFFFFFF),
    surfaceContainerLow = Color(0xFFFFFFFF),
    surfaceContainer = Color(0xFFFFFFFF),
    surfaceContainerHigh = Color(0xFFFFFFFF),
    surfaceContainerHighest = Color(0xFFF1F5F9),
    onBackground = Color(0xFF0F172A),
    onSurface = Color(0xFF0F172A),
    onSurfaceVariant = Color(0xFF475569),
    outline = Color(0xFF64748B),
    outlineVariant = Color(0xFFE2E8F0),
    primary = Color(0xFF4F46E5),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFE0E7FF),
    onPrimaryContainer = Color(0xFF3730A3),
    secondaryContainer = Color(0xFFE0E7FF),
    onSecondaryContainer = Color(0xFF3730A3),
    error = Color(0xFFB91C1C),
    errorContainer = Color(0xFFFEF2F2),
    onErrorContainer = Color(0xFFB91C1C),
    inverseSurface = Color(0xFF1E293B),
    inverseOnSurface = Color(0xFFF1F5F9),
    inversePrimary = Color(0xFFA5B4FC),
    scrim = Color(0x730F172A),
)

private val Dark = darkColorScheme(
    background = Color(0xFF0F172A),
    surface = Color(0xFF0F172A),
    surfaceContainerLowest = Color(0xFF1E293B),
    surfaceContainerLow = Color(0xFF1E293B),
    surfaceContainer = Color(0xFF1E293B),
    surfaceContainerHigh = Color(0xFF1E293B),
    surfaceContainerHighest = Color(0xFF334155),
    onBackground = Color(0xFFF1F5F9),
    onSurface = Color(0xFFF1F5F9),
    onSurfaceVariant = Color(0xFFCBD5E1),
    outline = Color(0xFF94A3B8),
    outlineVariant = Color(0xFF334155),
    primary = Color(0xFFA5B4FC),
    onPrimary = Color(0xFF1E1B4B),
    primaryContainer = Color(0xFF312E81),
    onPrimaryContainer = Color(0xFFE0E7FF),
    secondaryContainer = Color(0xFF312E81),
    onSecondaryContainer = Color(0xFFE0E7FF),
    error = Color(0xFFFCA5A5),
    errorContainer = Color(0x4D7F1D1D),
    onErrorContainer = Color(0xFFFCA5A5),
    inverseSurface = Color(0xFFE2E8F0),
    inverseOnSurface = Color(0xFF0F172A),
    inversePrimary = Color(0xFF4338CA),
    scrim = Color(0x99020617),
)

/** What Material 3 has no slot for (spec section 1.1, `TmColors`). */
@Immutable
data class TmColors(
    val fill: Color = Color(0xFF4F46E5),
    val onFill: Color = Color.White,
    val success: Color,
    val onSuccess: Color,
    val successContainer: Color,
    val errorStrong: Color,
    val onErrorStrong: Color,
    val danger: Color = Color(0xFFDC2626),
    val onDanger: Color = Color.White,
    val errorBorder: Color,
    val outlinedBorder: Color,
    val spark: Color = Color(0xFFF59E0B),
    val sparkGlow: Color = Color(0x66F59E0B),
)

private val LightTm = TmColors(
    success = Color(0xFF047857),
    onSuccess = Color.White,
    successContainer = Color(0xFFD1FAE5),
    errorStrong = Color(0xFFB91C1C),
    onErrorStrong = Color.White,
    errorBorder = Color(0xFFFECACA),
    outlinedBorder = Color(0xFFCBD5E1),
)

private val DarkTm = TmColors(
    success = Color(0xFF6EE7B7),
    onSuccess = Color(0xFF064E3B),
    successContainer = Color(0x66065F46),
    errorStrong = Color(0xFFFCA5A5),
    onErrorStrong = Color(0xFF450A0A),
    errorBorder = Color(0x99B91C1C),
    outlinedBorder = Color(0xFF475569),
)

val LocalTm = staticCompositionLocalOf { LightTm }

object Tm {
    val colors: TmColors @Composable get() = LocalTm.current
}

val Outfit = FontFamily(
    Font(R.font.outfit_regular, FontWeight.Normal),
    Font(R.font.outfit_medium, FontWeight.Medium),
    Font(R.font.outfit_semibold, FontWeight.SemiBold),
)

private fun style(size: Int, line: Int, weight: FontWeight) = TextStyle(
    fontFamily = Outfit,
    fontSize = size.sp,
    lineHeight = line.sp,
    fontWeight = weight,
    // Tabular figures, so counts and times line up.
    fontFeatureSettings = "tnum",
)

private val Type = Typography(
    displaySmall = style(34, 40, FontWeight.SemiBold),
    headlineSmall = style(24, 30, FontWeight.SemiBold),
    titleLarge = style(22, 28, FontWeight.Medium),
    titleMedium = style(20, 26, FontWeight.SemiBold),
    titleSmall = style(16, 22, FontWeight.SemiBold),
    bodyLarge = style(16, 22, FontWeight.Medium),
    bodyMedium = style(14, 20, FontWeight.Normal),
    bodySmall = style(13, 16, FontWeight.Normal),
    labelLarge = style(14, 20, FontWeight.SemiBold),
    labelMedium = style(12, 16, FontWeight.Medium),
    labelSmall = style(11, 16, FontWeight.Medium),
)

private val Shape = Shapes(
    extraSmall = RoundedCornerShape(8.dp),
    small = RoundedCornerShape(8.dp),
    medium = RoundedCornerShape(12.dp),
    large = RoundedCornerShape(16.dp),
    extraLarge = RoundedCornerShape(24.dp),
)

@Composable
fun TeyvatTheme(darkTheme: Boolean, content: @Composable () -> Unit) {
    val scheme: ColorScheme = if (darkTheme) Dark else Light
    CompositionLocalProvider(LocalTm provides if (darkTheme) DarkTm else LightTm) {
        MaterialTheme(colorScheme = scheme, typography = Type, shapes = Shape, content = content)
    }
}
