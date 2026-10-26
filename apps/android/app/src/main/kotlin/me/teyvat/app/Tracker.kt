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
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.ServerSocket
import java.net.SocketTimeoutException
import java.net.URL
import java.net.URLDecoder
import java.net.URLEncoder
import java.security.KeyStore
import java.security.MessageDigest
import java.security.SecureRandom
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
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** Who the tracker says this sign-in is (`whoami`), as the card shows it. */
data class Whoami(
    val user: String,
    /** `uid`: uploads go to the account matching the capture's UID. */
    val scope: String,
    val accountName: String?,
    val dashboardUrl: String,
    val configUrl: String,
) {
    companion object {
        fun parse(o: JSONObject?): Whoami? = o?.let {
            Whoami(
                user = it.optString("user"),
                scope = it.optString("scope", "uid"),
                accountName = if (it.isNull("accountName")) null else it.optString("accountName"),
                dashboardUrl = it.optString("dashboardUrl"),
                configUrl = it.optString("configUrl"),
            )
        }
    }
}

/** The last upload, for the card's line. */
data class UploadResult(val at: Long, val ok: Boolean, val text: String)

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
    private val uploadingFlow = MutableStateFlow(false)
    val uploading: StateFlow<Boolean> = uploadingFlow.asStateFlow()

    private val lock = Mutex()
    private var access: String? = null
    private var accessUntil = 0L
    private var signIn: Job? = null
    private var lastSignature: List<Long?>? = null

    init {
        Whoami.parse(store.getString("whoami", null)?.let { runCatching { JSONObject(it) }.getOrNull() })?.let { who ->
            if (readRefresh() != null) stateFlow.value = TrackerState.SignedIn(who)
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
        val o = getJson("${Links.TRACKER_API}/oauth/irminsul", null)
        val api = URL(Links.TRACKER_API)
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
        store.edit().putString(
            "whoami",
            JSONObject().put("user", who.user).put("scope", who.scope).put("accountName", who.accountName)
                .put("dashboardUrl", who.dashboardUrl).put("configUrl", who.configUrl).toString(),
        ).apply()
        stateFlow.value = TrackerState.SignedIn(who)
    }

    /** An access token, refreshed (and the rotated refresh token kept) when it is old. */
    private suspend fun accessToken(): String = lock.withLock {
        access?.takeIf { System.currentTimeMillis() < accessUntil }?.let { return it }
        val refresh = readRefresh() ?: throw SignedOut()
        val tokenUrl = store.getString("token_url", null) ?: "${Links.TRACKER_API}/oauth/token"
        val tokens = try {
            postJson(
                tokenUrl,
                JSONObject().put("grant_type", "refresh_token").put("client_id", CLIENT_ID).put("refresh_token", refresh),
                null,
            )
        } catch (e: HttpError) {
            if (e.status == 400 || e.status == 401) {
                forget()
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
            runCatching { withBearer { token -> getJson("${Links.TRACKER_API}/irminsul/whoami", token) } }
                .onSuccess { o -> Whoami.parse(o)?.let { withContext(Dispatchers.Main) { setWho(it) } } }
        }
    }

    fun signOut() {
        val refresh = readRefresh()
        val revokeUrl = store.getString("revoke_url", null) ?: "${Links.TRACKER_API}/oauth/revoke"
        forget()
        NativeBridge.note("info", "Signed out of ${Links.trackerHost}")
        if (refresh != null) {
            scope.launch(Dispatchers.IO) {
                runCatching { postJson(revokeUrl, JSONObject().put("token", refresh).put("client_id", CLIENT_ID), null) }
            }
        }
    }

    private fun forget() {
        access = null
        accessUntil = 0
        store.edit().remove("refresh").remove("whoami").apply()
        stateFlow.value = TrackerState.SignedOut
    }

    // -- uploads ------------------------------------------------------------------

    /**
     * Like the auto-export: once characters and items are captured and 5 s
     * after the last new data, the export goes to the tracker, once per
     * change.
     */
    fun tick(summary: Summary) {
        val settings = prefs.value
        if (!settings.trackerUpload || stateFlow.value !is TrackerState.SignedIn) return
        val dataAt = summary.dataAt ?: return
        if (summary.updatedCharacters == null || summary.updatedItems == null) return
        val signature = listOf(summary.login, summary.updatedItems, summary.updatedCharacters, summary.updatedAchievements)
        if (signature == lastSignature || uploadingFlow.value) return
        if (System.currentTimeMillis() - dataAt < Exports.DEBOUNCE_MS) return
        lastSignature = signature
        upload(dataAt)
    }

    /** Upload now (the card's button); false when nothing is captured. */
    fun uploadNow(summary: Summary): Boolean {
        val dataAt = summary.dataAt ?: return false
        if (uploadingFlow.value) return true
        lastSignature = listOf(summary.login, summary.updatedItems, summary.updatedCharacters, summary.updatedAchievements)
        upload(dataAt)
        return true
    }

    private fun upload(capturedAt: Long) {
        uploadingFlow.value = true
        scope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching {
                    val json = Exports.good(ExportFilters()) ?: throw IOException("Nothing to upload")
                    withBearer { token -> postGood("${Links.TRACKER_API}/irminsul/import", token, json, capturedAt) }
                }
            }
            uploadingFlow.value = false
            val now = System.currentTimeMillis()
            val done = result.fold(
                onSuccess = { UploadResult(now, true, context.getString(R.string.tracker_uploaded, hhmm(now))) },
                onFailure = { e ->
                    lastSignature = null
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

    private fun uploadError(e: Throwable): String = when (e) {
        is SignedOut -> context.getString(R.string.tracker_upload_failed, context.getString(R.string.teyvat_not_connected))
        is HttpError -> context.getString(R.string.tracker_upload_failed, "HTTP ${e.status} ${e.code ?: ""}".trim())
        else -> context.getString(R.string.tracker_upload_failed, e.message ?: e.javaClass.simpleName)
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

    private class HttpError(val status: Int, val code: String?, body: String) : IOException("HTTP $status $body")
    private class SignedOut : IOException("signed out")

    private fun open(url: String, method: String, bearer: String?): HttpURLConnection =
        (URL(url).openConnection() as HttpURLConnection).apply {
            requestMethod = method
            connectTimeout = 15_000
            readTimeout = 60_000
            setRequestProperty("User-Agent", "Teyvatme-Android/${BuildConfig.VERSION_NAME}")
            setRequestProperty("Accept", "application/json")
            bearer?.let { setRequestProperty("Authorization", "Bearer $it") }
        }

    private fun answer(connection: HttpURLConnection): JSONObject {
        val status = connection.responseCode
        val stream = if (status in 200..299) connection.inputStream else connection.errorStream
        val body = stream?.bufferedReader()?.use { it.readText() }.orEmpty()
        if (status !in 200..299) {
            val o = runCatching { JSONObject(body) }.getOrNull()
            val code = o?.optJSONObject("error")?.optString("code")?.takeIf { it.isNotEmpty() }
                ?: o?.optString("error")?.takeIf { it.isNotEmpty() }
            throw HttpError(status, code, body.take(200))
        }
        return runCatching { JSONObject(body) }.getOrDefault(JSONObject())
    }

    private fun getJson(url: String, bearer: String?): JSONObject {
        val c = open(url, "GET", bearer)
        try {
            return answer(c)
        } finally {
            c.disconnect()
        }
    }

    private fun postJson(url: String, body: JSONObject, bearer: String?): JSONObject {
        val c = open(url, "POST", bearer)
        try {
            c.doOutput = true
            c.setRequestProperty("Content-Type", "application/json")
            c.outputStream.use { it.write(body.toString().toByteArray()) }
            return answer(c)
        } finally {
            c.disconnect()
        }
    }

    /** The desktop's `snapshot_form`: the capture time first, then the file. */
    private fun postGood(url: String, bearer: String, json: String, capturedAt: Long): JSONObject {
        val boundary = "teyvatme" + randomToken(16)
        val body = ByteArrayOutputStream()
        fun line(s: String) = body.write((s + "\r\n").toByteArray())
        line("--$boundary")
        line("Content-Disposition: form-data; name=\"timestamp\"")
        line("")
        line(capturedAt.toString())
        line("--$boundary")
        line("Content-Disposition: form-data; name=\"file\"; filename=\"teyvatme_capture.json\"")
        line("Content-Type: application/json")
        line("")
        body.write(json.toByteArray())
        line("")
        line("--$boundary--")
        val c = open(url, "POST", bearer)
        try {
            c.doOutput = true
            c.readTimeout = 120_000
            c.setRequestProperty("Content-Type", "multipart/form-data; boundary=$boundary")
            c.setFixedLengthStreamingMode(body.size())
            c.outputStream.use { body.writeTo(it) }
            return answer(c)
        } finally {
            c.disconnect()
        }
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

    private fun writeRefresh(token: String) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        val sealed = cipher.iv + cipher.doFinal(token.toByteArray())
        store.edit().putString("refresh", Base64.encodeToString(sealed, Base64.NO_WRAP)).commit()
    }

    private fun readRefresh(): String? = runCatching {
        val sealed = Base64.decode(store.getString("refresh", null) ?: return null, Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, sealed, 0, 12))
        String(cipher.doFinal(sealed, 12, sealed.size - 12))
    }.getOrNull()

    companion object {
        private const val TAG = "teyvatme"
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
