// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch
import org.json.JSONObject

/** The tunnel, as the service last reported it. */
enum class VpnState { Off, Starting, On }

/** What the home screen's hero shows (spec §4.1.1). */
enum class Hero {
    CaptureOff, Starting, Waiting, GameDetected, GameAlreadyOpen, LoginSeen, Decrypting, MissedAlreadyRunning,
    MissedCaptureStopped, KeyLost;

    val isProblem get() = this == MissedAlreadyRunning || this == MissedCaptureStopped || this == KeyLost
}

data class Counts(
    val characters: Int = 0,
    val weapons: Int = 0,
    val artifacts: Int = 0,
    val materials: Int = 0,
    val achievements: Int = 0,
)

data class Player(val ar: Int?, val wl: Int?, val server: String?)

/** Reading the wish history into a UIGF file (native `wish::Read`). */
data class WishRead(
    /** `idle`, `running`, `done` or `failed`. */
    val state: String = "idle",
    val banner: Int = 0,
    val page: Int = 0,
    val records: Int = 0,
    val at: Long = 0,
    val error: String? = null,
)

/** The wish card (native `wish::WishSummary`). */
data class WishInfo(
    /** `none`, `ready` or `expired`. */
    val key: String = "none",
    val uidKnown: Boolean = false,
    val read: WishRead = WishRead(),
)

data class ActivityEvent(val at: Long, val kind: String, val text: String) {
    val isError get() = kind == "error"
}

/** The native decoder's summary (`decoder::Summary`). */
data class Summary(
    val ready: Boolean = false,
    val error: String? = null,
    val phase: String = "waiting",
    val missedByRestart: Boolean = false,
    /** The game's connection began before this capture. */
    val alreadyOpen: Boolean = false,
    val keyAt: Long? = null,
    val dataVersion: Long = 0,
    val updatedItems: Long? = null,
    val updatedCharacters: Long? = null,
    val updatedAchievements: Long? = null,
    val dataAt: Long? = null,
    val login: Long = 0,
    val counts: Counts = Counts(),
    val player: Player? = null,
    val gameData: String? = null,
    val events: List<ActivityEvent> = emptyList(),
    val wish: WishInfo = WishInfo(),
) {
    val captured get() = dataAt != null

    companion object {
        fun parse(json: String): Summary {
            val o = runCatching { JSONObject(json) }.getOrNull() ?: return Summary()
            fun JSONObject.long(key: String): Long? = if (isNull(key) || !has(key)) null else optLong(key)
            fun JSONObject.int(key: String): Int? = if (isNull(key) || !has(key)) null else optInt(key)
            val updated = o.optJSONObject("updated")
            val counts = o.optJSONObject("counts")
            val player = o.optJSONObject("player")
            val events = o.optJSONArray("events")
            val wish = o.optJSONObject("wish")
            val read = wish?.optJSONObject("read")
            return Summary(
                ready = o.optBoolean("ready"),
                error = if (o.isNull("error")) null else o.optString("error"),
                phase = o.optString("phase", "waiting"),
                missedByRestart = o.optBoolean("missedByRestart"),
                alreadyOpen = o.optBoolean("alreadyOpen"),
                keyAt = o.long("keyAt"),
                dataVersion = o.optLong("dataVersion"),
                updatedItems = updated?.long("items"),
                updatedCharacters = updated?.long("characters"),
                updatedAchievements = updated?.long("achievements"),
                dataAt = o.long("dataAt"),
                login = o.optLong("login"),
                counts = Counts(
                    characters = counts?.optInt("characters") ?: 0,
                    weapons = counts?.optInt("weapons") ?: 0,
                    artifacts = counts?.optInt("artifacts") ?: 0,
                    materials = counts?.optInt("materials") ?: 0,
                    achievements = counts?.optInt("achievements") ?: 0,
                ),
                player = player?.let {
                    Player(it.int("ar"), it.int("wl"), if (it.isNull("server")) null else it.optString("server"))
                },
                gameData = if (o.isNull("gameData")) null else o.optString("gameData"),
                wish = WishInfo(
                    key = wish?.optString("key", "none") ?: "none",
                    uidKnown = wish?.optBoolean("uidKnown") ?: false,
                    read = WishRead(
                        state = read?.optString("state", "idle") ?: "idle",
                        banner = read?.optInt("banner") ?: 0,
                        page = read?.optInt("page") ?: 0,
                        records = read?.optInt("records") ?: 0,
                        at = read?.optLong("at") ?: 0,
                        error = read?.takeIf { it.has("error") }?.optString("error"),
                    ),
                ),
                events = buildList {
                    for (i in 0 until (events?.length() ?: 0)) {
                        val e = events!!.getJSONObject(i)
                        add(ActivityEvent(e.optLong("at"), e.optString("kind"), e.optString("text")))
                    }
                },
            )
        }
    }
}

/** The hero for the tunnel's state and the decoder's phase. */
fun hero(vpn: VpnState, summary: Summary): Hero = when (vpn) {
    VpnState.Off -> Hero.CaptureOff
    VpnState.Starting -> Hero.Starting
    VpnState.On -> when (summary.phase) {
        "gameDetected" -> if (summary.alreadyOpen) Hero.GameAlreadyOpen else Hero.GameDetected
        "loginSeen" -> Hero.LoginSeen
        "decrypting" -> Hero.Decrypting
        "launchMissed" -> if (summary.missedByRestart) Hero.MissedCaptureStopped else Hero.MissedAlreadyRunning
        "keyLost" -> Hero.KeyLost
        else -> Hero.Waiting
    }
}

/**
 * The capture as the app sees it: the tunnel's state from the service and
 * the decoder's summary, read once a second while capture runs or a screen
 * shows it. [onRefresh] sees every read (the auto-export), without being a
 * watcher that would keep the polling alive.
 */
class Capture(private val scope: CoroutineScope, private val onRefresh: (Summary) -> Unit) {
    val vpn = MutableStateFlow(VpnState.Off)
    /** When the tunnel last began (re)starting, for Starting's Open Activity. */
    var startingSince = 0L
        private set
    private val summaryState = MutableStateFlow(Summary())
    val summary: StateFlow<Summary> = summaryState.asStateFlow()
    private var poller: Job? = null

    init {
        scope.launch {
            combine(vpn, summaryState.subscriptionCount) { state, watchers -> state != VpnState.Off || watchers > 0 }
                .distinctUntilChanged()
                .collect { wanted ->
                    poller?.cancel()
                    poller = if (wanted) scope.launch { poll() } else null
                }
        }
    }

    fun setVpn(state: VpnState) {
        if (state == VpnState.Starting && vpn.value != VpnState.Starting) startingSince = System.currentTimeMillis()
        val was = vpn.value
        vpn.value = state
        refresh()
        if (state == VpnState.Off && was != VpnState.Off) Updater.instance?.onCaptureStopped()
    }

    fun refresh() {
        val summary = Summary.parse(NativeBridge.summary())
        summaryState.value = summary
        onRefresh(summary)
    }

    private suspend fun poll() {
        while (true) {
            refresh()
            delay(1_000)
        }
    }
}
