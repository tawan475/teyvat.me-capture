// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import androidx.annotation.StringRes

/** The Sharing screen's presets. */
enum class SharePreset { Everything, Builds, Custom }

/**
 * One part of the export a tracker upload can leave out (Settings ›
 * Sharing). [key] is its name in the upload's `gi_off` list: a contract
 * with the tracker, the same as the desktop's (`teyvatme_core::sharing`),
 * and the order here is that list's.
 */
enum class SharePart(val key: String, @StringRes val title: Int, @StringRes val line: Int) {
    Characters("characters", R.string.share_part_characters, R.string.share_part_characters_line),
    Weapons("weapons", R.string.share_part_weapons, R.string.share_part_weapons_line),
    Artifacts("artifacts", R.string.share_part_artifacts, R.string.share_part_artifacts_line),
    Materials("materials", R.string.share_part_materials, R.string.share_part_materials_line),
    Profile("gi_player.profile", R.string.share_part_profile, R.string.share_part_profile_line),
    Numbers("gi_player.numbers", R.string.share_part_numbers, R.string.share_part_numbers_line),
    CharacterExtras("gi_characters", R.string.share_part_extras, R.string.share_part_extras_line),
    Achievements("gi_achievements", R.string.share_part_achievements, R.string.share_part_achievements_line),
    Anecdotes("gi_anecdotes", R.string.share_part_anecdotes, R.string.share_part_anecdotes_line),
    Tasks("gi_tasks", R.string.share_part_tasks, R.string.share_part_tasks_line),
    Endgame("gi_endgame", R.string.share_part_endgame, R.string.share_part_endgame_line),
    Exploration("gi_exploration", R.string.share_part_exploration, R.string.share_part_exploration_line),
    Collections("gi_collections", R.string.share_part_collections, R.string.share_part_collections_line),
    Wardrobe("gi_wardrobe", R.string.share_part_wardrobe, R.string.share_part_wardrobe_line),
    Teapot("gi_teapot", R.string.share_part_teapot, R.string.share_part_teapot_line),
    Events("gi_events", R.string.share_part_events, R.string.share_part_events_line),
    Miliastra("gi_miliastra", R.string.share_part_miliastra, R.string.share_part_miliastra_line),
    ;

    companion object {
        /** The screen's groups, in order: their name and parts. */
        val GROUPS: List<Pair<Int, List<SharePart>>> = listOf(
            R.string.share_group_builds to listOf(Characters, Weapons, Artifacts, Materials),
            R.string.share_group_account to listOf(Profile, Numbers),
            R.string.share_group_progress to listOf(CharacterExtras, Achievements, Anecdotes, Tasks, Endgame, Exploration, Collections),
            R.string.share_group_home to listOf(Wardrobe, Teapot, Events, Miliastra),
        )

        /** "Builds only": the four build sections and the account numbers, nothing else. */
        val BUILDS_ONLY_OFF: Set<String> =
            entries.filter { it !in GROUPS[0].second && it != Numbers }.map { it.key }.toSet()

        /** The parts that are off, in the contract's order (unknown names dropped). */
        fun off(off: Set<String>): List<SharePart> = entries.filter { it.key in off }

        /** [off] for [NativeBridge.setSharing]. */
        fun names(off: Set<String>): String = off(off).joinToString(",") { it.key }

        /** The preset a set of switches is: a fixed one when it equals theirs (unless Custom was picked). */
        fun preset(off: Set<String>, customPicked: Boolean): SharePreset {
            val set = off(off).map { it.key }.toSet()
            return when {
                customPicked -> SharePreset.Custom
                set.isEmpty() -> SharePreset.Everything
                set == BUILDS_ONLY_OFF -> SharePreset.Builds
                else -> SharePreset.Custom
            }
        }

        /**
         * Whether uploads are placed by the capture's UID: signed in with
         * scope `uid` (or a scope not known yet), or with a key that is not
         * one account's (`gdt_import_…`). No way to upload counts as by UID:
         * sending the UID is what uploads always did.
         */
        fun routesByUid(scope: String?, signedIn: Boolean, key: String?): Boolean = when {
            signedIn -> scope != "account"
            else -> key?.startsWith("gdt_import_") != true
        }
    }
}
