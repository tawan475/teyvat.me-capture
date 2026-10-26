// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

package me.teyvat.app

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Environment
import android.os.PersistableBundle
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.util.Log
import androidx.core.content.FileProvider
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Why an export could not be made. */
sealed class ExportProblem {
    data object NothingYet : ExportProblem()

    /** Classes the filters need that were not captured: their tiles' names. */
    data class Missing(val what: List<Int>) : ExportProblem()
}

/** GOOD JSON out of the decoder and onto the phone: files, the share sheet, the clipboard. */
object Exports {
    private const val TAG = "teyvatme"
    private const val FOLDER = "Teyvat.me"
    /** The desktop's automation debounce: this long after the last new data. */
    const val DEBOUNCE_MS = 5_000L

    /** `teyvatme_wishes_<yyyy-MM-dd_HH-mm>.json`: a UIGF file. */
    fun wishFileName(at: Long = System.currentTimeMillis()): String =
        "teyvatme_wishes_" + SimpleDateFormat("yyyy-MM-dd_HH-mm", Locale.ROOT).format(Date(at)) + ".json"

    /** `teyvatme_<yyyy-MM-dd_HH-mm>.json`. */
    fun fileName(at: Long = System.currentTimeMillis()): String =
        "teyvatme_" + SimpleDateFormat("yyyy-MM-dd_HH-mm", Locale.ROOT).format(Date(at)) + ".json"

    /** What the filters need that is not captured yet; null when it can export. */
    fun problem(summary: Summary, filters: ExportFilters): ExportProblem? {
        if (!summary.captured) return ExportProblem.NothingYet
        val missing = buildList {
            if (filters.includeCharacters && summary.updatedCharacters == null) add(R.string.tile_characters)
            if (summary.updatedItems == null) {
                if (filters.includeWeapons) add(R.string.tile_weapons)
                if (filters.includeArtifacts) add(R.string.tile_artifacts)
                if (filters.includeMaterials) add(R.string.tile_materials)
            }
        }
        return if (missing.isEmpty()) null else ExportProblem.Missing(missing)
    }

    fun good(filters: ExportFilters): String? = NativeBridge.exportGood(filters.toJson())

    fun write(context: Context, uri: Uri, json: String, truncate: Boolean = true) {
        context.contentResolver.openOutputStream(uri, if (truncate) "wt" else "w").use { out ->
            requireNotNull(out) { "cannot open $uri" }.write(json.toByteArray())
        }
    }

    /** The GOOD JSON as a file in the cache, for the share sheet. */
    fun shareIntent(context: Context, json: String, name: String = fileName()): Intent {
        val dir = File(context.cacheDir, "share").apply { mkdirs() }
        dir.listFiles()?.forEach { it.delete() }
        val file = File(dir, name)
        file.writeText(json)
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
        val send = Intent(Intent.ACTION_SEND)
            .setType("application/json")
            .putExtra(Intent.EXTRA_STREAM, uri)
            .putExtra(Intent.EXTRA_TITLE, file.name)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        send.clipData = ClipData.newRawUri(file.name, uri)
        return Intent.createChooser(send, null)
    }

    /**
     * The logs for a bug report: latest.log, previous.log and the last
     * crash, whichever exist. They hold the account's numbers (Mora and the
     * like) but never a key or a wish URL.
     */
    fun shareLogIntent(context: Context): Intent? {
        val files = listOf("latest.log", "previous.log", "crash.txt")
            .map { File(context.filesDir, it) }
            .filter { it.exists() && it.length() > 0 }
        if (files.isEmpty()) return null
        val uris = ArrayList(files.map { FileProvider.getUriForFile(context, "${context.packageName}.files", it) })
        val send = Intent(Intent.ACTION_SEND_MULTIPLE)
            .setType("text/plain")
            .putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        val clip = ClipData.newRawUri(files[0].name, uris[0])
        uris.drop(1).forEach { clip.addItem(ClipData.Item(it)) }
        send.clipData = clip
        return Intent.createChooser(send, null)
    }

    /**
     * Copies [text]; false when the clipboard refused it (a GOOD JSON of a
     * couple of MB can exceed the binder's limit).
     */
    fun copy(context: Context, label: String, text: String, sensitive: Boolean = false): Boolean {
        val clipboard = context.getSystemService(ClipboardManager::class.java)
        val clip = ClipData.newPlainText(label, text)
        if (sensitive) {
            clip.description.extras = PersistableBundle().apply {
                putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
            }
        }
        return try {
            clipboard.setPrimaryClip(clip)
            true
        } catch (e: RuntimeException) {
            Log.w(TAG, "copy of ${text.length} chars refused: $e")
            false
        }
    }

    /** A new file in the export folder: the chosen tree, or Download/Teyvat.me. */
    fun createInFolder(context: Context, settings: Settings, name: String): Uri? {
        val tree = settings.exportTree?.let(Uri::parse)
        if (tree != null) {
            val parent = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
            return DocumentsContract.createDocument(context.contentResolver, parent, "application/json", name)
        }
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, name)
            put(MediaStore.MediaColumns.MIME_TYPE, "application/json")
            put(MediaStore.MediaColumns.RELATIVE_PATH, "${Environment.DIRECTORY_DOWNLOADS}/$FOLDER")
        }
        return context.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
    }

    /**
     * A document's file name. Its URI says nothing a person can read (the
     * Downloads provider's ends in an id), so ask the provider.
     */
    fun displayName(context: Context, uri: Uri): String? = runCatching {
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) cursor.getString(0) else null
        }
    }.getOrNull()

    /** The folder's name as the rows show it. */
    fun folderLabel(context: Context, settings: Settings): String {
        val tree = settings.exportTree?.let(Uri::parse) ?: return "Download/$FOLDER"
        val id = runCatching { DocumentsContract.getTreeDocumentId(tree) }.getOrNull() ?: return tree.toString()
        return id.substringAfter(':').ifEmpty { id }
    }

    /** Opens the export folder in the Files app; false when nothing could. */
    fun openFolder(context: Context, settings: Settings): Boolean {
        val tree = settings.exportTree?.let(Uri::parse)
        val folder = if (tree != null) {
            DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
        } else {
            DocumentsContract.buildDocumentUri(
                "com.android.externalstorage.documents",
                "primary:${Environment.DIRECTORY_DOWNLOADS}/$FOLDER",
            )
        }
        val view = Intent(Intent.ACTION_VIEW)
            .setDataAndType(folder, DocumentsContract.Document.MIME_TYPE_DIR)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION)
        return try {
            context.startActivity(view)
            true
        } catch (_: ActivityNotFoundException) {
            try {
                context.startActivity(
                    Intent(android.app.DownloadManager.ACTION_VIEW_DOWNLOADS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                )
                true
            } catch (_: ActivityNotFoundException) {
                false
            }
        }
    }

    /** Opens one exported file with whatever reads JSON. */
    fun openFile(context: Context, uri: Uri): Boolean {
        val view = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, "application/json")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        return try {
            context.startActivity(Intent.createChooser(view, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            true
        } catch (_: ActivityNotFoundException) {
            false
        }
    }
}

/**
 * The desktop's automation, for files: once everything the filters need is
 * captured, and 5 s after the last new data, the GOOD JSON goes to the export
 * folder. One file per login: later changes in the same login rewrite it,
 * so a session of small updates leaves one file, not dozens.
 */
class AutoExporter(private val context: Context, private val prefs: Prefs, private val scope: CoroutineScope) {
    private var lastSignature: List<Long?>? = null
    private var busy = false

    fun tick(summary: Summary) {
        val settings = prefs.value
        if (!settings.autoExport || !settings.onboarded) return
        val dataAt = summary.dataAt ?: return
        if (Exports.problem(summary, settings.filters) != null) return
        val signature = listOf(summary.login, summary.updatedItems, summary.updatedCharacters, summary.updatedAchievements)
        if (signature == lastSignature) return
        if (busy || System.currentTimeMillis() - dataAt < Exports.DEBOUNCE_MS) return
        lastSignature = signature
        busy = true
        scope.launch {
            withContext(Dispatchers.IO) { export(settings, summary) }
            busy = false
        }
    }

    private fun export(settings: Settings, summary: Summary) {
        val json = Exports.good(settings.filters) ?: return
        val last = settings.lastAutoExport
        val now = System.currentTimeMillis()
        val written = runCatching {
            if (last != null && last.login == summary.login && last.at > (summary.keyAt ?: 0)) {
                runCatching { Exports.write(context, Uri.parse(last.uri), json) }
                    .map { last.copy(at = now) }
                    .getOrNull()
            } else {
                null
            } ?: run {
                val name = Exports.fileName(now)
                val uri = requireNotNull(Exports.createInFolder(context, settings, name)) { "the folder refused a new file" }
                Exports.write(context, uri, json)
                SavedFile(name, uri.toString(), now, summary.login)
            }
        }
        written.onSuccess { file ->
            prefs.update { it.copy(lastAutoExport = file) }
            NativeBridge.note("ok", "Auto-exported ${file.name}")
        }.onFailure { e ->
            Log.e("teyvatme", "auto-export failed", e)
            NativeBridge.note("error", "Auto-export failed · ${e.message ?: e.javaClass.simpleName}")
        }
    }
}
