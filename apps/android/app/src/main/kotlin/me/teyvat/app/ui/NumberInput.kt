// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * A number in [range]: − and + step by one (held, they repeat, faster after
 * a while), and the number itself is a field to type into, kept in range
 * when it loses focus or on Done. [format] shows it while not typing
 * ("C2", "+20", "4★"); a tap empties the field (the number stays as a
 * hint, and leaving it empty keeps it), so the digits typed replace it.
 */
@Composable
fun NumberStepper(value: Int, range: IntRange, onChange: (Int) -> Unit, format: (Int) -> String = { "$it" }, fieldWidth: Dp = 52.dp) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        RepeatButton("−", "−1", value, value > range.first, onChange) { (it - 1).coerceIn(range) }
        NumberField(value, range, onChange, format, fieldWidth)
        RepeatButton("+", "+1", value, value < range.last, onChange) { (it + 1).coerceIn(range) }
    }
}

/** Held: the first repeat 450 ms after the press, then every 300 ms down to 40 ms. */
private fun repeatDelay(step: Int): Long = (300L - step * 40L).coerceAtLeast(40L)

@Composable
private fun RepeatButton(label: String, description: String, value: Int, enabled: Boolean, onChange: (Int) -> Unit, next: (Int) -> Int) {
    val current by rememberUpdatedState(value)
    val commit by rememberUpdatedState(onChange)
    val scope = rememberCoroutineScope()
    val color = if (enabled) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline
    Box(
        Modifier
            .size(44.dp)
            .clip(CircleShape)
            .semantics {
                role = Role.Button
                contentDescription = description
                if (!enabled) disabled()
                onClick { commit(next(current)); true }
            }
            .pointerInput(enabled) {
                if (!enabled) return@pointerInput
                detectTapGestures(onPress = {
                    // A local count: the settings round trip may not have
                    // recomposed between two repeats.
                    var shown = next(current)
                    commit(shown)
                    val job: Job = scope.launch {
                        delay(450)
                        var step = 0
                        while (true) {
                            val again = next(shown)
                            if (again == shown) break
                            shown = again
                            commit(shown)
                            delay(repeatDelay(step++))
                        }
                    }
                    tryAwaitRelease()
                    job.cancel()
                })
            },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = MaterialTheme.typography.titleMedium, color = color)
    }
}

@Composable
private fun NumberField(value: Int, range: IntRange, onChange: (Int) -> Unit, format: (Int) -> String, width: Dp) {
    val scheme = MaterialTheme.colorScheme
    val focus = LocalFocusManager.current
    var typing by remember { mutableStateOf(false) }
    var text by remember { mutableStateOf(TextFieldValue("")) }
    // Empty while typing, the current number as a hint, so what is typed
    // replaces it. The tap that focused the field still hands back the old
    // text (to place its cursor), so it is emptied again a frame later.
    LaunchedEffect(typing) {
        if (typing) {
            withFrameNanos {}
            withFrameNanos {}
            text = TextFieldValue("")
        }
    }
    BasicTextField(
        value = if (typing) text else TextFieldValue(format(value)),
        onValueChange = { if (typing) text = it.copy(text = it.text.filter(Char::isDigit).take(4)) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number, imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = { focus.clearFocus() }),
        textStyle = MaterialTheme.typography.bodyLarge.copy(color = scheme.onSurface, textAlign = TextAlign.Center),
        cursorBrush = SolidColor(scheme.primary),
        modifier = Modifier
            .width(width)
            .height(36.dp)
            .border(1.dp, if (typing) scheme.primary else scheme.outlineVariant, MaterialTheme.shapes.small)
            .onFocusChanged { state ->
                if (state.isFocused && !typing) {
                    text = TextFieldValue("")
                    typing = true
                } else if (!state.isFocused && typing) {
                    typing = false
                    text.text.toIntOrNull()?.let { typed -> onChange(typed.coerceIn(range)) }
                }
            },
        decorationBox = { inner ->
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                if (typing && text.text.isEmpty()) {
                    Text("$value", style = MaterialTheme.typography.bodyLarge, color = scheme.outline)
                }
                inner()
            }
        },
    )
}
