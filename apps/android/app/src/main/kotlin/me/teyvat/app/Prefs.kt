// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.content.Context
import android.content.SharedPreferences
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONObject

enum class ThemeMode { System, Light, Dark }

/**
 * What a GOOD JSON export includes: the desktop's `ExportSettings` (its
 * Genshin Optimizer settings), with the same defaults. Tracker uploads
 * always include everything; these are only for the files.
 */
data class ExportFilters(
    val includeCharacters: Boolean = true,
    val includeArtifacts: Boolean = true,
    val includeWeapons: Boolean = true,
    val includeMaterials: Boolean = true,
    val fakeInitialize4thLine: Boolean = false,
    val minCharacterLevel: Int = 1,
    val minCharacterAscension: Int = 0,
    val minCharacterConstellation: Int = 0,
    val minArtifactLevel: Int = 0,
    val minArtifactRarity: Int = 3,
    val minWeaponLevel: Int = 1,
    val minWeaponRefinement: Int = 0,
    val minWeaponAscension: Int = 0,
    val minWeaponRarity: Int = 3,
) {
    /** The native `ExportSettings` JSON. */
    fun toJson(): String = JSONObject()
        .put("include_characters", includeCharacters)
        .put("include_artifacts", includeArtifacts)
        .put("include_weapons", includeWeapons)
        .put("include_materials", includeMaterials)
        .put("fake_initialize_4th_line", fakeInitialize4thLine)
        .put("min_character_level", minCharacterLevel)
        .put("min_character_ascension", minCharacterAscension)
        .put("min_character_constellation", minCharacterConstellation)
        .put("min_artifact_level", minArtifactLevel)
        .put("min_artifact_rarity", minArtifactRarity)
        .put("min_weapon_level", minWeaponLevel)
        .put("min_weapon_refinement", minWeaponRefinement)
        .put("min_weapon_ascension", minWeaponAscension)
        .put("min_weapon_rarity", minWeaponRarity)
        .toString()

    /** The data classes these filters need captured: characters, items. */
    fun needsCharacters() = includeCharacters
    fun needsItems() = includeArtifacts || includeWeapons || includeMaterials

    companion object {
        fun fromJson(json: String?): ExportFilters {
            val o = json?.let { runCatching { JSONObject(it) }.getOrNull() } ?: return ExportFilters()
            val d = ExportFilters()
            return ExportFilters(
                includeCharacters = o.optBoolean("include_characters", d.includeCharacters),
                includeArtifacts = o.optBoolean("include_artifacts", d.includeArtifacts),
                includeWeapons = o.optBoolean("include_weapons", d.includeWeapons),
                includeMaterials = o.optBoolean("include_materials", d.includeMaterials),
                fakeInitialize4thLine = o.optBoolean("fake_initialize_4th_line", d.fakeInitialize4thLine),
                minCharacterLevel = o.optInt("min_character_level", d.minCharacterLevel),
                minCharacterAscension = o.optInt("min_character_ascension", d.minCharacterAscension),
                minCharacterConstellation = o.optInt("min_character_constellation", d.minCharacterConstellation),
                minArtifactLevel = o.optInt("min_artifact_level", d.minArtifactLevel),
                minArtifactRarity = o.optInt("min_artifact_rarity", d.minArtifactRarity),
                minWeaponLevel = o.optInt("min_weapon_level", d.minWeaponLevel),
                minWeaponRefinement = o.optInt("min_weapon_refinement", d.minWeaponRefinement),
                minWeaponAscension = o.optInt("min_weapon_ascension", d.minWeaponAscension),
                minWeaponRarity = o.optInt("min_weapon_rarity", d.minWeaponRarity),
            )
        }
    }
}

/** The last file the auto-export wrote, for its row and Open export folder. */
data class SavedFile(val name: String, val uri: String, val at: Long, val login: Long)

data class Settings(
    /** First run is done once the VPN was allowed. */
    val onboarded: Boolean = false,
    val theme: ThemeMode = ThemeMode.System,
    /** On by default after first run, to Download/Teyvat.me. */
    val autoExport: Boolean = true,
    /** A folder chosen with the document picker; null = Download/Teyvat.me. */
    val exportTree: String? = null,
    val filters: ExportFilters = ExportFilters(),
    /** Game packets to a pcap for a bug report. Personal data, never uploaded. */
    val recordCapture: Boolean = false,
    val lastAutoExport: SavedFile? = null,
    /** The last manual Save, for Open export folder when auto-export is off. */
    val lastSave: String? = null,
    /** Signed in to the tracker: upload each capture. */
    val trackerUpload: Boolean = true,
    /** Signed in to the tracker: sync the wish history when the game sends its key. */
    val sendWishes: Boolean = true,
    /** Between snapshots, send what changed as you play (the tracker's switch decides too). */
    val liveSync: Boolean = true,
    /** Download and install new releases by themselves. */
    val autoUpdate: Boolean = true,
)

/** The app's settings, in SharedPreferences, as one observable value. */
class Prefs(context: Context) {
    private val store: SharedPreferences = context.getSharedPreferences("settings", Context.MODE_PRIVATE)
    private val state = MutableStateFlow(read())
    val settings: StateFlow<Settings> = state.asStateFlow()
    val value: Settings get() = state.value

    fun update(change: (Settings) -> Settings) {
        val next = change(state.value)
        state.value = next
        write(next)
    }

    private fun read(): Settings {
        val last = store.getString("last_auto_export", null)?.let { json ->
            runCatching {
                val o = JSONObject(json)
                SavedFile(o.getString("name"), o.getString("uri"), o.getLong("at"), o.optLong("login"))
            }.getOrNull()
        }
        return Settings(
            onboarded = store.getBoolean("onboarded", false),
            theme = runCatching { ThemeMode.valueOf(store.getString("theme", null) ?: "") }
                .getOrDefault(ThemeMode.System),
            autoExport = store.getBoolean("auto_export", true),
            exportTree = store.getString("export_tree", null),
            filters = ExportFilters.fromJson(store.getString("filters", null)),
            recordCapture = store.getBoolean("record_capture", false),
            lastAutoExport = last,
            lastSave = store.getString("last_save", null),
            trackerUpload = store.getBoolean("tracker_upload", true),
            sendWishes = store.getBoolean("send_wishes", true),
            liveSync = store.getBoolean("live_sync", true),
            autoUpdate = store.getBoolean("auto_update", true),
        )
    }

    private fun write(s: Settings) {
        store.edit()
            .putBoolean("onboarded", s.onboarded)
            .putString("theme", s.theme.name)
            .putBoolean("auto_export", s.autoExport)
            .putString("export_tree", s.exportTree)
            .putString("filters", s.filters.toJson())
            .putBoolean("record_capture", s.recordCapture)
            .putString(
                "last_auto_export",
                s.lastAutoExport?.let {
                    JSONObject().put("name", it.name).put("uri", it.uri).put("at", it.at).put("login", it.login).toString()
                },
            )
            .putString("last_save", s.lastSave)
            .putBoolean("tracker_upload", s.trackerUpload)
            .putBoolean("send_wishes", s.sendWishes)
            .putBoolean("live_sync", s.liveSync)
            .putBoolean("auto_update", s.autoUpdate)
            .apply()
    }
}
