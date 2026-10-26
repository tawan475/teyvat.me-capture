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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import me.teyvat.app.R
import me.teyvat.app.Settings

@Composable
fun FanLine(modifier: Modifier = Modifier) {
    Text(
        stringResource(R.string.about_fan_project),
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.outline,
        textAlign = TextAlign.Center,
        modifier = modifier.fillMaxWidth(),
    )
}

@Composable
fun WelcomeScreen(padding: PaddingValues, onStart: () -> Unit) {
    Column(
        Modifier
            .fillMaxSize()
            .padding(padding)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 24.dp),
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        Spacer(Modifier.height(24.dp))
        Spark(56)
        Text(stringResource(R.string.app_name), style = MaterialTheme.typography.displaySmall)
        Text(stringResource(R.string.welcome_tagline), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
        TmCard {
            ListRow(stringResource(R.string.welcome_only_genshin), stringResource(R.string.welcome_only_genshin_line), R.drawable.ic_shield_check)
            CardDivider()
            ListRow(stringResource(R.string.welcome_read_only), stringResource(R.string.welcome_read_only_line), R.drawable.ic_eye)
            CardDivider()
            ListRow(stringResource(R.string.welcome_on_phone), stringResource(R.string.welcome_on_phone_line), R.drawable.ic_folder)
            CardDivider()
            ListRow(stringResource(R.string.welcome_before_game), stringResource(R.string.welcome_before_game_line), R.drawable.ic_power)
        }
        Spacer(Modifier.height(8.dp))
        TmButton(stringResource(R.string.get_started), onStart, large = true)
        FanLine()
    }
}

/** What Set up's buttons do. */
interface SetUpActions {
    fun setAutoExport(on: Boolean)
    fun chooseFolder()
    fun openVpnSettings()
    fun notificationsAllowed(): Boolean
    fun allowNotifications()
    fun allowBackground()
    fun startCapture()
}

@Composable
fun SetUpScreen(
    padding: PaddingValues,
    settings: Settings,
    folderLabel: String,
    notificationsAllowed: Boolean,
    backgroundAllowed: Boolean,
    actions: SetUpActions,
) {
    Column(
        Modifier
            .fillMaxSize()
            .padding(padding)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(stringResource(R.string.setup_title), style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(vertical = 8.dp))
        TmCard {
            SwitchRow(stringResource(R.string.export_auto), folderLabel, settings.autoExport, actions::setAutoExport, icon = R.drawable.ic_folder)
            CardDivider()
            ListRow(
                stringResource(R.string.setup_choose_folder), stringResource(R.string.setup_choose_folder_line),
                onClick = actions::chooseFolder,
            ) { TmIcon(R.drawable.ic_chevron_right, null) }
        }
        TmCard {
            ListRow(stringResource(R.string.always_on_vpn), stringResource(R.string.always_on_vpn_line), R.drawable.ic_power)
            Text(
                stringResource(R.string.always_on_vpn_path),
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline,
                modifier = Modifier.padding(start = 56.dp, end = 16.dp),
            )
            Row(Modifier.padding(start = 56.dp, top = 8.dp, bottom = 14.dp)) {
                TmButton(stringResource(R.string.open_vpn_settings), actions::openVpnSettings, kind = ButtonKind.Tonal, trailingIcon = R.drawable.ic_external_link)
            }
        }
        if (!notificationsAllowed) {
            TmCard {
                ListRow(stringResource(R.string.notifications), stringResource(R.string.notifications_line), R.drawable.ic_bell)
                Row(Modifier.padding(start = 56.dp, bottom = 14.dp)) {
                    TmButton(stringResource(R.string.allow), actions::allowNotifications, kind = ButtonKind.Tonal)
                }
            }
        }
        if (!backgroundAllowed) {
            TmCard {
                ListRow(stringResource(R.string.battery_title), stringResource(R.string.battery_off_line), R.drawable.ic_power)
                Row(Modifier.padding(start = 56.dp, bottom = 14.dp)) {
                    TmButton(stringResource(R.string.allow), actions::allowBackground, kind = ButtonKind.Tonal)
                }
            }
        }
        Spacer(Modifier.height(12.dp))
        TmButton(stringResource(R.string.home_start_capture), actions::startCapture, large = true, icon = R.drawable.ic_power)
        Text(
            stringResource(R.string.setup_vpn_dialog), style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline, textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth(),
        )
    }
}
