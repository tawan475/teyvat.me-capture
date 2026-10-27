// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.content.Context
import android.content.SharedPreferences
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import java.net.SocketTimeoutException
import java.net.URL
import java.net.URLDecoder
import java.net.URLEncoder
import java.security.KeyStore
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.concurrent.TimeUnit
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.MultipartBody
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject

/** Who the tracker says this sign-in is (`whoami`), as the card shows it. */
data class Whoami(
    val user: String,
    /** `uid`: uploads go to the account matching the capture's UID. */
    val scope: String,
    val accountName: String?,
    val dashboardUrl: String,
    val configUrl: String,
    /** The upload account's id and UID (`scope == "account"`); no id = it was deleted. */
    val accountId: String? = null,
    val uid: String? = null,
) {
    companion object {
        fun parse(o: JSONObject?): Whoami? = o?.let {
            Whoami(
                user = it.optString("user"),
                scope = it.optString("scope", "uid"),
                accountName = if (it.isNull("accountName")) null else it.optString("accountName"),
                dashboardUrl = it.optString("dashboardUrl"),
                configUrl = it.optString("configUrl"),
                accountId = it.opt("accountId")?.takeIf { v -> v != JSONObject.NULL }?.toString()?.takeIf { v -> v.isNotEmpty() },
                uid = it.opt("uid")?.takeIf { v -> v != JSONObject.NULL }?.toString()?.trim()?.takeIf { v -> v.isNotEmpty() },
            )
        }
    }
}

/** The last upload, for the card's line. */
/** The import key's account, as `verify-key` names it. */
data class KeyAccount(val name: String, val uid: String)

/** The import key (Settings › Tracker › Advanced), used only while signed out. */
data class ImportKey(
    val key: String? = null,
    val checking: Boolean = false,
    val account: KeyAccount? = null,
) {
    val verified get() = key != null && account != null
}

data class UploadResult(val at: Long, val ok: Boolean, val text: String)

/** The last live sync batch the tracker stored, for the Tracker row's line. */
data class LiveResult(val at: Long, val changes: Int)

sealed class TrackerState {
    data object SignedOut : TrackerState()
    /** The browser is open on the tracker's sign-in page. */
    data object SigningIn : TrackerState()
    data class SignedIn(val who: Whoami) : TrackerState()
}

/**
 * Sign in with teyvat.me and upload each capture: the desktop's
 * `signin.rs` contract (OAuth 2.0 code + PKCE, a loopback redirect on
 * 127.0.0.1, refresh tokens that rotate on every use). The refresh token is
 * kept encrypted with a key in the Android keystore; the access token only
 * in memory. Uploads are the whole GOOD export, like the desktop's.
 */
class Tracker(private val context: Context, private val prefs: Prefs, private val scope: CoroutineScope) {
    private val store: SharedPreferences = context.getSharedPreferences("tracker", Context.MODE_PRIVATE)
    private val stateFlow = MutableStateFlow<TrackerState>(TrackerState.SignedOut)
    val state: StateFlow<TrackerState> = stateFlow.asStateFlow()
    private val lastFlow = MutableStateFlow<UploadResult?>(null)
    val lastUpload: StateFlow<UploadResult?> = lastFlow.asStateFlow()
    private val revokedFlow = MutableStateFlow(false)
    /** The tracker ended this sign-in (signed out on the website): "Signed out, sign in again". */
    val revoked: StateFlow<Boolean> = revokedFlow.asStateFlow()
    private val keyFlow = MutableStateFlow(ImportKey())
    /** The import key and whether the tracker accepted it. */
    val importKey: StateFlow<ImportKey> = keyFlow.asStateFlow()
    private val uploadingFlow = MutableStateFlow(false)
    val uploading: StateFlow<Boolean> = uploadingFlow.asStateFlow()
    private val liveFlow = MutableStateFlow<LiveResult?>(null)
    /** The last stored live sync batch (this process only). */
    val lastLive: StateFlow<LiveResult?> = liveFlow.asStateFlow()
    private val signInUrlFlow = MutableStateFlow<String?>(null)
    /** The sign-in page while the browser is open on it, for Copy link. */
    val signInUrl: StateFlow<String?> = signInUrlFlow.asStateFlow()

    private val lock = Mutex()
    private var access: String? = null
    private var accessUntil = 0L
    private var signIn: Job? = null
    private var lastSignature: List<Long?>? = null

    init {
        Whoami.parse(store.getString("whoami", null)?.let { runCatching { JSONObject(it) }.getOrNull() })?.let { who ->
            if (readRefresh() != null) stateFlow.value = TrackerState.SignedIn(who)
        }
        revokedFlow.value = store.getBoolean("revoked", false) && readRefresh() == null
        unseal("import_key")?.let { key ->
            val account = store.getString("key_account", null)?.let { json ->
                runCatching { JSONObject(json).let { KeyAccount(it.getString("name"), it.getString("uid")) } }.getOrNull()
            }
            keyFlow.value = ImportKey(key, account = account)
        }
        store.getString("last_upload", null)?.let { json ->
            runCatching {
                val o = JSONObject(json)
                lastFlow.value = UploadResult(o.getLong("at"), o.getBoolean("ok"), o.getString("text"))
            }
        }
    }

    // -- sign-in ----------------------------------------------------------------

    /** Opens the tracker's sign-in page with [openUrl]; [onDone] brings the app back. */
    fun signIn(openUrl: (String) -> Unit, onDone: (error: String?) -> Unit) {
        signIn?.cancel()
        stateFlow.value = TrackerState.SigningIn
        signIn = scope.launch {
            val error = withContext(Dispatchers.IO) {
                runCatching { runSignIn(openUrl) }.exceptionOrNull()
            }
            signInUrlFlow.value = null
            if (!isActive) return@launch
            if (error != null) {
                Log.w(TAG, "sign-in failed: $error")
                stateFlow.value = signedInOrOut()
                onDone(error.message ?: error.javaClass.simpleName)
            } else {
                onDone(null)
            }
        }
    }

    fun cancelSignIn() {
        signIn?.cancel()
        signIn = null
        signInUrlFlow.value = null
        stateFlow.value = signedInOrOut()
    }

    private fun signedInOrOut(): TrackerState {
        val who = (stateFlow.value as? TrackerState.SignedIn)?.who
            ?: Whoami.parse(store.getString("whoami", null)?.let { runCatching { JSONObject(it) }.getOrNull() })
        return if (who != null && readRefresh() != null) TrackerState.SignedIn(who) else TrackerState.SignedOut
    }

    private suspend fun runSignIn(openUrl: (String) -> Unit) {
        val discovery = discover()
        val verifier = randomToken(48)
        val challenge = base64Url(MessageDigest.getInstance("SHA-256").digest(verifier.toByteArray()))
        val state = randomToken(24)
        ServerSocket(0, 4, InetAddress.getByName("127.0.0.1")).use { server ->
            server.soTimeout = 1_000
            val redirect = "http://127.0.0.1:${server.localPort}/callback"
            val url = discovery.authorize + "?" + query(
                "client_id" to CLIENT_ID,
                "redirect_uri" to redirect,
                "state" to state,
                "code_challenge" to challenge,
                "code_challenge_method" to "S256",
                "device" to deviceName(),
                "app" to CLIENT_APP,
            )
            signInUrlFlow.value = url
            withContext(Dispatchers.Main) { openUrl(url) }
            val deadline = System.currentTimeMillis() + SIGN_IN_TIMEOUT_MS
            var code: String? = null
            while (code == null) {
                if (System.currentTimeMillis() > deadline) throw IOException("The sign-in took too long")
                kotlinx.coroutines.yield()
                val socket = try {
                    server.accept()
                } catch (_: SocketTimeoutException) {
                    continue
                }
                socket.use {
                    it.soTimeout = 5_000
                    val line = it.getInputStream().bufferedReader().readLine().orEmpty()
                    val target = line.split(' ').getOrNull(1).orEmpty()
                    val path = target.substringBefore('?')
                    val params = parseQuery(target.substringAfter('?', ""))
                    val out = it.getOutputStream()
                    if (path != "/callback") {
                        out.write(httpAnswer("404 Not Found", ""))
                        return@use
                    }
                    if (params["state"] != state) {
                        out.write(httpAnswer("400 Bad Request", page(context.getString(R.string.tracker_page_wrong))))
                        return@use
                    }
                    params["error"]?.let { e ->
                        out.write(httpAnswer("200 OK", page(context.getString(R.string.tracker_page_cancelled))))
                        throw IOException(if (e == "access_denied") "Sign-in cancelled" else e)
                    }
                    code = params["code"]
                    out.write(httpAnswer("200 OK", page(context.getString(R.string.tracker_page_done))))
                }
            }
            val tokens = postJson(
                discovery.token,
                JSONObject()
                    .put("grant_type", "authorization_code")
                    .put("client_id", CLIENT_ID)
                    .put("app", CLIENT_APP)
                    .put("code", code)
                    .put("code_verifier", verifier)
                    .put("redirect_uri", redirect),
                null,
            )
            lock.withLock { keep(tokens, discovery) }
            NativeBridge.note("ok", context.getString(R.string.tracker_signed_in_toast, Links.trackerHost))
            lastSignature = null
        }
    }

    private class Discovery(val authorize: String, val token: String, val revoke: String)

    private fun discover(): Discovery {
        val o = getJson("${Links.trackerApi}/oauth/irminsul", null)
        val api = URL(Links.trackerApi)
        val token = o.getString("token_url")
        val revoke = o.getString("revoke_url")
        // The token and revoke URLs must be the API's own (the contract's rule).
        for (u in listOf(token, revoke)) {
            val url = URL(u)
            if (url.protocol != api.protocol || url.host != api.host || url.port != api.port) {
                throw IOException("The tracker's sign-in answer pointed elsewhere")
            }
        }
        store.edit().putString("token_url", token).putString("revoke_url", revoke).apply()
        return Discovery(o.getString("authorize_url"), token, revoke)
    }

    /** Stores a token answer: the refresh token first, then the access token in memory. */
    private fun keep(tokens: JSONObject, discovery: Discovery? = null) {
        writeRefresh(tokens.getString("refresh_token"))
        access = tokens.getString("access_token")
        accessUntil = System.currentTimeMillis() + tokens.optLong("expires_in", 900) * 1000 - 30_000
        Whoami.parse(tokens.optJSONObject("whoami"))?.let(::setWho)
        if (discovery != null) {
            store.edit().putString("token_url", discovery.token).putString("revoke_url", discovery.revoke).apply()
        }
    }

    private fun setWho(who: Whoami) {
        if (revokedFlow.value) {
            revokedFlow.value = false
            store.edit().putBoolean("revoked", false).apply()
        }
        store.edit().putString(
            "whoami",
            JSONObject().put("user", who.user).put("scope", who.scope).put("accountName", who.accountName)
                .put("dashboardUrl", who.dashboardUrl).put("configUrl", who.configUrl)
                .put("accountId", who.accountId).put("uid", who.uid).toString(),
        ).apply()
        stateFlow.value = TrackerState.SignedIn(who)
    }

    /** An access token, refreshed (and the rotated refresh token kept) when it is old. */
    private suspend fun accessToken(): String = lock.withLock {
        access?.takeIf { System.currentTimeMillis() < accessUntil }?.let { return it }
        val refresh = readRefresh() ?: throw SignedOut()
        val tokenUrl = store.getString("token_url", null) ?: "${Links.trackerApi}/oauth/token"
        val tokens = try {
            postJson(
                tokenUrl,
                JSONObject().put("grant_type", "refresh_token").put("client_id", CLIENT_ID).put("refresh_token", refresh),
                null,
            )
        } catch (e: HttpError) {
            if (e.status == 400 || e.status == 401) {
                forget(revoked = true)
                throw SignedOut()
            }
            throw e
        }
        keep(tokens)
        access!!
    }

    /** whoami again (the upload target may have changed on the website). */
    fun refreshWho() {
        if (stateFlow.value !is TrackerState.SignedIn) return
        scope.launch(Dispatchers.IO) {
            runCatching { withBearer { token -> getJson("${Links.trackerApi}/irminsul/whoami", token) } }
                .onSuccess { o -> Whoami.parse(o)?.let { withContext(Dispatchers.Main) { setWho(it) } } }
        }
    }

    fun signOut() {
        val refresh = readRefresh()
        val revokeUrl = store.getString("revoke_url", null) ?: "${Links.trackerApi}/oauth/revoke"
        forget()
        NativeBridge.note("info", "Signed out of ${Links.trackerHost}")
        if (refresh != null) {
            scope.launch(Dispatchers.IO) {
                runCatching { postJson(revokeUrl, JSONObject().put("token", refresh).put("client_id", CLIENT_ID), null) }
            }
        }
    }

    private fun forget(revoked: Boolean = false) {
        access = null
        accessUntil = 0
        store.edit().remove("refresh").remove("whoami").putBoolean("revoked", revoked).apply()
        stateFlow.value = TrackerState.SignedOut
        revokedFlow.value = revoked
    }

    // -- uploads ------------------------------------------------------------------

    /** The automated upload's last signature that failed, and when it is tried again. */
    private var retryAt = 0L
    private var backoff = 0L
    /** A live sync batch is on its way. */
    @Volatile
    private var batchInFlight = false

    /**
     * Like the auto-export: once characters and items are captured and 5 s
     * after the last new data, the tracker's snapshot goes up, once per
     * change (in live mode only at login and world changes: batches carry
     * the rest). Then live sync sends what it decided (`teyvatme_core::live`).
     */
    fun tick(summary: Summary) {
        val settings = prefs.value
        val uploadsOn = canUpload() && settings.trackerUpload
        val live = uploadsOn && settings.liveSync
        NativeBridge.setWishSync(uploadsOn && settings.sendWishes)
        noteWishes(summary.wish.sync)
        if (!uploadsOn) return
        liveTick(live)
        val dataAt = summary.dataAt ?: return
        if (summary.updatedCharacters == null || summary.updatedItems == null) return
        val signature = listOf(summary.login, summary.updatedItems, summary.updatedCharacters, summary.updatedAchievements)
        if (uploadingFlow.value) return
        val now = System.currentTimeMillis()
        if (signature == lastSignature) {
            // A failed upload is tried again after a wait, not every second.
            if (retryAt == 0L || now < retryAt) return
        } else {
            retryAt = 0L
            backoff = 0L
        }
        if (now - dataAt < Exports.DEBOUNCE_MS) return
        lastSignature = signature
        upload(dataAt, manual = false, live = live)
    }

    /** Upload now (the card's button); false when nothing is captured. */
    fun uploadNow(summary: Summary): Boolean {
        val dataAt = summary.dataAt ?: return false
        if (uploadingFlow.value) return true
        lastSignature = listOf(summary.login, summary.updatedItems, summary.updatedCharacters, summary.updatedAchievements)
        retryAt = 0L
        val settings = prefs.value
        upload(dataAt, manual = true, live = settings.trackerUpload && settings.liveSync)
        return true
    }

    private var notedWishSync = 0L

    /** "N new wishes sent to the tracker", once per sync that added some. */
    private fun noteWishes(sync: WishSync) {
        val last = sync.last ?: return
        if (last.at == notedWishSync) return
        notedWishSync = last.at
        if (last.added > 0) NativeBridge.note("ok", context.getString(R.string.wish_new_sent, grouped(last.added.toInt())))
    }

    /**
     * One request of the native wish sync ([WishTrackerBridge]), signed in:
     * `"<status>\n<body>"`. Blocks; never on the main thread.
     */
    fun wishCall(method: String, path: String, body: String): String = runBlocking(Dispatchers.IO) {
        if (!canUpload()) return@runBlocking "-1\n"
        try {
            "200\n" + withCredential { credential -> rawCall(method, "${Links.trackerApi}$path", body, credential) }
        } catch (e: SignedOut) {
            "-1\n"
        } catch (e: HttpError) {
            if (e.status == 401 && e.code == "session_revoked") forget(revoked = true)
            "${e.status}\n${e.body}"
        } catch (e: IOException) {
            "0\n" + (e.message ?: e.javaClass.simpleName)
        }
    }

    /** A snapshot: the automated upload, Upload now, or one live sync asked for ([liveSnapshot]: its ticket and JSON). */
    private fun upload(capturedAt: Long, manual: Boolean, live: Boolean, liveSnapshot: Pair<Long, String>? = null) {
        uploadingFlow.value = true
        scope.launch {
            var ticket = 0L
            var skipped = false
            val result = withContext(Dispatchers.IO) {
                runCatching {
                    val (t, json) = liveSnapshot ?: run {
                        val export = NativeBridge.trackerExport(manual, live) ?: throw IOException("Nothing to upload")
                        if (export.isEmpty()) {
                            skipped = true
                            return@runCatching JSONObject()
                        }
                        export.substringBefore('\n').toLong() to export.substringAfter('\n')
                    }
                    ticket = t
                    withCredential { credential -> postGood(credential, json, capturedAt) }
                }
            }
            uploadingFlow.value = false
            // In live mode the batches carry this change.
            if (skipped) return@launch
            NativeBridge.snapshotAnswered(ticket, result.getOrNull()?.toString())
            val now = System.currentTimeMillis()
            val done = result.fold(
                onSuccess = {
                    retryAt = 0L
                    backoff = 0L
                    UploadResult(now, true, context.getString(R.string.tracker_uploaded, hhmm(now)))
                },
                onFailure = { e ->
                    if (liveSnapshot == null) {
                        backoff = if (backoff == 0L) FIRST_RETRY_MS else (backoff * 2).coerceAtMost(MAX_RETRY_MS)
                        retryAt = now + backoff
                    }
                    if (e is HttpError && e.status == 401 && e.code == "session_revoked") forget(revoked = true)
                    UploadResult(now, false, uploadError(e))
                },
            )
            lastFlow.value = done
            store.edit().putString(
                "last_upload", JSONObject().put("at", done.at).put("ok", done.ok).put("text", done.text).toString(),
            ).apply()
            NativeBridge.note(if (done.ok) "ok" else "error", done.text)
        }
    }

    /** Send what live sync decided now: a batch, or a snapshot it asked for. */
    private fun liveTick(live: Boolean) {
        if (uploadingFlow.value || batchInFlight) return
        val next = NativeBridge.liveNextTick(live)
        if (next < 0 || System.currentTimeMillis() < next) return
        val action = NativeBridge.liveTick(live) ?: return
        when {
            action.startsWith("S") -> {
                val ticket = action.substring(1).substringBefore('\n').toLongOrNull() ?: return
                upload(System.currentTimeMillis(), manual = false, live = live, liveSnapshot = ticket to action.substringAfter('\n'))
            }
            action.startsWith("B") -> sendBatch(action.substringAfter('\n'))
        }
    }

    /** POST one GOOD-X batch to `/irminsul/changes`; the answer goes back to live sync. */
    private fun sendBatch(delta: String) {
        batchInFlight = true
        scope.launch {
            val (status, body) = withContext(Dispatchers.IO) {
                try {
                    200 to withCredential { credential -> rawCall("POST", "${Links.trackerApi}/irminsul/changes", delta, credential) }
                } catch (e: SignedOut) {
                    -1 to ""
                } catch (e: HttpError) {
                    if (e.status == 401 && e.code == "session_revoked") forget(revoked = true)
                    e.status to e.body
                } catch (e: IOException) {
                    0 to ""
                }
            }
            batchInFlight = false
            NativeBridge.batchAnswered(status, body)
            val stored = status in 200..299 && runCatching { JSONObject(body).optBoolean("ok") && !JSONObject(body).optBoolean("duplicate") }.getOrDefault(false)
            if (stored) {
                val changes = runCatching { JSONObject(delta).optJSONArray("changes")?.length() ?: 0 }.getOrDefault(0)
                liveFlow.value = LiveResult(System.currentTimeMillis(), changes)
                NativeBridge.note("ok", context.getString(R.string.live_changes_sent, grouped(changes), Links.trackerHost))
            }
        }
    }

    private fun uploadError(e: Throwable): String = when (e) {
        is SignedOut -> context.getString(R.string.tracker_upload_failed, context.getString(R.string.teyvat_not_connected))
        is HttpError -> context.getString(R.string.tracker_upload_failed, "HTTP ${e.status} ${e.code ?: ""}".trim())
        else -> context.getString(R.string.tracker_upload_failed, e.message ?: e.javaClass.simpleName)
    }

    // -- the import key and the address -------------------------------------------

    /** Keep [key] (encrypted, like the refresh token) and ask the tracker whose it is. */
    fun setImportKey(key: String) {
        val trimmed = key.trim()
        if (trimmed.isEmpty()) return clearImportKey()
        seal("import_key", trimmed)
        store.edit().remove("key_account").apply()
        keyFlow.value = ImportKey(trimmed)
        verifyKey()
    }

    fun clearImportKey() {
        store.edit().remove("import_key").remove("key_account").apply()
        keyFlow.value = ImportKey()
    }

    /** `GET /genshin-accounts-public/verify-key`: the key's account, or invalid. */
    fun verifyKey() {
        val key = keyFlow.value.key ?: return
        keyFlow.value = keyFlow.value.copy(checking = true)
        scope.launch {
            val account = withContext(Dispatchers.IO) {
                runCatching {
                    val o = getJsonAs("${Links.trackerApi}/genshin-accounts-public/verify-key", Credential.Key(key))
                        .let { it.optJSONObject("data") ?: it }
                    KeyAccount(
                        o.optString("accountName").ifEmpty { "?" },
                        o.opt("uid")?.takeIf { it != JSONObject.NULL }?.toString() ?: "N/A",
                    )
                }.getOrNull()
            }
            if (keyFlow.value.key != key) return@launch
            keyFlow.value = ImportKey(key, account = account)
            store.edit().putString(
                "key_account", account?.let { JSONObject().put("name", it.name).put("uid", it.uid).toString() },
            ).apply()
        }
    }

    /**
     * Uploads go to [api] from now on. A sign-in belongs to one address, so
     * it ends; the import key is checked again there.
     */
    fun setApi(api: String) {
        val base = api.trim().trimEnd('/')
        if (base == Links.trackerApi) return
        if (stateFlow.value is TrackerState.SignedIn) signOut()
        store.edit().putString("api", base.takeIf { it != Links.DEFAULT_TRACKER_API }).remove("token_url").remove("revoke_url").apply()
        lastSignature = null
        verifyKey()
    }

    /** How a request proves who it uploads for: the sign-in, else a verified import key (the desktop's `upload_credential`). */
    private sealed class Credential {
        class Bearer(val token: String) : Credential()
        class Key(val key: String) : Credential()
    }

    /** Whether uploads can go anywhere: signed in, or a verified import key. */
    fun canUpload() = stateFlow.value is TrackerState.SignedIn || keyFlow.value.verified

    /** Runs [call] with the sign-in, else the verified import key. */
    private suspend fun <T> withCredential(call: (Credential) -> T): T {
        if (stateFlow.value is TrackerState.SignedIn) return withBearer { call(Credential.Bearer(it)) }
        val key = keyFlow.value.takeIf { it.verified }?.key ?: throw SignedOut()
        return try {
            call(Credential.Key(key))
        } catch (e: HttpError) {
            // A key the tracker no longer takes: check it again.
            if (e.status == 401 || e.status == 403) withContext(Dispatchers.Main) { verifyKey() }
            throw e
        }
    }

    /** Runs [call] with an access token; a 401 refreshes once and tries again. */
    private suspend fun <T> withBearer(call: (String) -> T): T {
        val token = accessToken()
        return try {
            call(token)
        } catch (e: HttpError) {
            if (e.status != 401) throw e
            lock.withLock { access = null }
            call(accessToken())
        }
    }

    // -- HTTP ---------------------------------------------------------------------

    private class HttpError(val status: Int, val code: String?, val body: String) : IOException("HTTP $status ${body.take(200)}")
    private class SignedOut : IOException("signed out")

    /** A request to the tracker ([Net.client]); the body of a success, else [HttpError]. */
    private fun send(url: String, method: String, credential: Credential?, body: RequestBody? = null, readSeconds: Long = 60): String {
        val request = Request.Builder()
            .url(url)
            .method(method, body)
            .header("User-Agent", "Teyvatme-Android/${BuildConfig.VERSION_NAME}")
            .header("Accept", "application/json")
            .apply {
                when (credential) {
                    is Credential.Bearer -> header("Authorization", "Bearer ${credential.token}")
                    is Credential.Key -> header("x-import-key", credential.key)
                    null -> {}
                }
            }
            .build()
        val client = if (readSeconds == 60L) Net.client else Net.client.newBuilder().readTimeout(readSeconds, TimeUnit.SECONDS).build()
        client.newCall(request).execute().use { response ->
            val text = response.body?.string().orEmpty()
            if (!response.isSuccessful) {
                val o = runCatching { JSONObject(text) }.getOrNull()
                val code = o?.optJSONObject("error")?.optString("code")?.takeIf { it.isNotEmpty() }
                    ?: o?.optString("error")?.takeIf { it.isNotEmpty() }
                throw HttpError(response.code, code, text)
            }
            return text
        }
    }

    private fun json(text: String) = runCatching { JSONObject(text) }.getOrDefault(JSONObject())

    /** A request with a JSON body (or none); the body of a success. */
    private fun rawCall(method: String, url: String, body: String, credential: Credential): String =
        send(url, method, credential, if (method == "GET") null else body.toRequestBody(JSON_TYPE))

    private fun getJson(url: String, bearer: String?): JSONObject = getJsonAs(url, bearer?.let { Credential.Bearer(it) })

    private fun getJsonAs(url: String, credential: Credential?): JSONObject = json(send(url, "GET", credential))

    private fun postJson(url: String, body: JSONObject, bearer: String?): JSONObject =
        json(send(url, "POST", bearer?.let { Credential.Bearer(it) }, body.toString().toRequestBody(JSON_TYPE)))

    /** The desktop's `snapshot_form`: the capture time first, then the file. */
    private fun postGood(credential: Credential, json: String, capturedAt: Long): JSONObject {
        // The import-key route is the old backend's, which irminsul's shipped binaries call.
        val url = when (credential) {
            is Credential.Bearer -> "${Links.trackerApi}/irminsul/import"
            is Credential.Key -> "${Links.trackerApi}/genshin-accounts-public/import-by-key"
        }
        val body = MultipartBody.Builder("teyvatme" + randomToken(16))
            .setType(MultipartBody.FORM)
            .addFormDataPart("timestamp", capturedAt.toString())
            .addFormDataPart("file", "teyvatme_capture.json", json.toRequestBody(JSON_TYPE))
            .build()
        return json(send(url, "POST", credential, body, readSeconds = 120))
    }

    // -- the refresh token, encrypted -----------------------------------------------

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return generator.generateKey()
    }

    private fun writeRefresh(token: String) = seal("refresh", token)

    private fun readRefresh(): String? = unseal("refresh")

    /** [value] encrypted under [name] with the keystore's key. */
    private fun seal(name: String, value: String) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        val sealed = cipher.iv + cipher.doFinal(value.toByteArray())
        store.edit().putString(name, Base64.encodeToString(sealed, Base64.NO_WRAP)).commit()
    }

    private fun unseal(name: String): String? = runCatching {
        val sealed = Base64.decode(store.getString(name, null) ?: return null, Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, sealed, 0, 12))
        String(cipher.doFinal(sealed, 12, sealed.size - 12))
    }.getOrNull()

    companion object {
        private const val TAG = "teyvatme"
        private val JSON_TYPE = "application/json".toMediaType()
        /** A failed automated upload is tried again after this, doubling up to [MAX_RETRY_MS]. */
        private const val FIRST_RETRY_MS = 30_000L
        private const val MAX_RETRY_MS = 600_000L
        /** The OAuth client id the tracker knows Teyvat.me by (shared with irminsul). */
        const val CLIENT_ID = "irminsul"
        /** Names the device "Teyvat.me on <phone>" on the tracker. */
        const val CLIENT_APP = "teyvatme"
        private const val KEY_ALIAS = "teyvatme_tracker"
        private const val SIGN_IN_TIMEOUT_MS = 10 * 60_000L

        /** The phone's name for the tracker's Devices list: "Pixel 8". */
        fun deviceName(): String {
            val model = Build.MODEL.orEmpty().trim()
            val maker = Build.MANUFACTURER.orEmpty().trim()
            val name = if (maker.isNotEmpty() && !model.startsWith(maker, ignoreCase = true)) "$maker $model" else model
            return name.filter { !it.isISOControl() }.take(64).ifBlank { "Android phone" }
        }

        private fun randomToken(bytes: Int): String = base64Url(ByteArray(bytes).also { SecureRandom().nextBytes(it) })

        private fun base64Url(bytes: ByteArray): String =
            Base64.encodeToString(bytes, Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP)

        private fun query(vararg pairs: Pair<String, String>): String =
            pairs.joinToString("&") { (k, v) -> "$k=" + URLEncoder.encode(v, "UTF-8") }

        private fun parseQuery(q: String): Map<String, String> = q.split('&').filter { it.isNotEmpty() }.associate {
            URLDecoder.decode(it.substringBefore('='), "UTF-8") to URLDecoder.decode(it.substringAfter('=', ""), "UTF-8")
        }

        private fun page(text: String) =
            "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'>" +
                "<title>Teyvat.me</title><body style='font:18px sans-serif;background:#0f172a;color:#e2e8f0;" +
                "display:grid;place-items:center;min-height:90vh;margin:0'><p>$text</p>"

        private fun httpAnswer(status: String, body: String): ByteArray {
            val bytes = body.toByteArray()
            return ("HTTP/1.1 $status\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: ${bytes.size}\r\n" +
                "Cache-Control: no-store\r\nConnection: close\r\n\r\n").toByteArray() + bytes
        }
    }
}
