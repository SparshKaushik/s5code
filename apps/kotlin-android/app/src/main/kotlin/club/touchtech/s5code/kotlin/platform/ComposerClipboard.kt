package club.touchtech.s5code.kotlin.platform

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.content.MediaType
import androidx.compose.foundation.content.consume
import androidx.compose.foundation.content.contentReceiver
import androidx.compose.foundation.content.hasMediaType
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import club.touchtech.s5code.kotlin.model.ComposerImageCandidate

/**
 * Clipboard, keyboard, drop, and picker intake for composer images.
 *
 * Everything here runs on the main thread (a paste gesture and an activity
 * result both land there), so it only reads the cheap parts: the clip's own mime
 * types and the URI. Display name, byte size, and the durable copy are resolved
 * off the main thread by [rememberComposerImageIntake].
 */

/** Images the OS clipboard is currently holding, as unvalidated candidates. */
fun readClipboardImageCandidates(context: Context): List<ComposerImageCandidate> {
    val clipboard = context.getSystemService(ClipboardManager::class.java) ?: return emptyList()
    val clip = clipboard.primaryClip ?: return emptyList()
    return imageCandidates(context.contentResolver, clip.description, clip)
}

/** Whether the clipboard holds anything the composer could attach as an image. */
fun clipboardHasImage(context: Context): Boolean {
    val clipboard = context.getSystemService(ClipboardManager::class.java) ?: return false
    val description = clipboard.primaryClipDescription ?: return false
    return (0 until description.mimeTypeCount).any {
        description.getMimeType(it).startsWith("image/")
    }
}

/**
 * Tracks whether the clipboard holds an image, so the explicit paste action can
 * hide or disable itself. Backed by the clip-changed callback plus a refresh on
 * resume, because a copy made by another app while this one is backgrounded
 * delivers no callback. Only the clip *description* is read, which does not trip
 * the system's "pasted from clipboard" notice.
 */
@Composable
fun rememberClipboardHasImage(): Boolean {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    var hasImage by remember(context) { mutableStateOf(false) }

    DisposableEffect(context, lifecycleOwner) {
        val clipboard = context.getSystemService(ClipboardManager::class.java)
        val refresh = { hasImage = clipboardHasImage(context) }
        val clipListener = ClipboardManager.OnPrimaryClipChangedListener { refresh() }
        val lifecycleObserver = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) refresh()
        }
        clipboard?.addPrimaryClipChangedListener(clipListener)
        lifecycleOwner.lifecycle.addObserver(lifecycleObserver)
        refresh()
        onDispose {
            clipboard?.removePrimaryClipChangedListener(clipListener)
            lifecycleOwner.lifecycle.removeObserver(lifecycleObserver)
        }
    }

    return hasImage
}

/** Reads the clipboard on demand, for an explicit paste affordance. */
@Composable
fun rememberClipboardImageReader(): () -> List<ComposerImageCandidate> {
    val context = LocalContext.current
    return remember(context) { { readClipboardImageCandidates(context) } }
}

/**
 * Photo-picker launcher bounded by the draft's remaining attachment slots. The
 * picker grants read access per URI, so no storage permission is involved.
 */
@Composable
fun rememberComposerImagePicker(
    remaining: Int,
    onImages: (List<ComposerImageCandidate>) -> Unit,
): () -> Unit {
    val resolver = LocalContext.current.contentResolver
    val slots = remaining.coerceAtLeast(0)
    // PickMultipleVisualMedia rejects a limit below 2, so a single free slot
    // falls back to the single-item contract.
    val multiple =
        rememberLauncherForActivityResult(
            ActivityResultContracts.PickMultipleVisualMedia(slots.coerceAtLeast(2))
        ) { uris ->
            onImages(uris.map { candidateFor(resolver, it) })
        }
    val single =
        rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
            onImages(listOfNotNull(uri?.let { candidateFor(resolver, it) }))
        }
    return {
        val request = PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)
        // A full draft still opens the picker: validation then names the limit,
        // which reads better than a button that silently does nothing.
        if (slots >= 2) multiple.launch(request) else single.launch(request)
    }
}

/**
 * Accepts images the keyboard commits (Gboard clipboard, sticker and GIF
 * insertion) and images dropped onto the field, handing them to [onImages].
 * Text and everything else falls through to the text field.
 *
 * Keyboard commits only reach the field that owns the input connection, so this
 * belongs on the text field rather than a wrapper around it.
 */
@OptIn(ExperimentalFoundationApi::class)
fun Modifier.composerImageReceiver(onImages: (List<ComposerImageCandidate>) -> Unit): Modifier =
    composed {
        val resolver = LocalContext.current.contentResolver
        contentReceiver { content ->
            if (!content.hasMediaType(MediaType.Image)) return@contentReceiver content
            val accepted = mutableListOf<ComposerImageCandidate>()
            val remaining =
                content.consume { item ->
                    val uri = item.uri ?: return@consume false
                    val mimeType = resolver.getType(uri)
                    if (mimeType?.startsWith("image/") != true) return@consume false
                    accepted += ComposerImageCandidate(uri = uri.toString(), mimeType = mimeType)
                    true
                }
            if (accepted.isNotEmpty()) onImages(accepted)
            remaining
        }
    }

/**
 * Candidate for every clip item that resolves to an image. Non-image items are
 * left alone, so a mixed clip still pastes its text through the text field.
 */
private fun imageCandidates(
    resolver: ContentResolver,
    description: ClipDescription?,
    clip: ClipData,
): List<ComposerImageCandidate> =
    (0 until clip.itemCount).mapNotNull { index ->
        val uri = clip.getItemAt(index).uri ?: return@mapNotNull null
        val mimeType =
            description?.takeIf { index < it.mimeTypeCount }?.getMimeType(index)?.takeIf {
                it.startsWith("image/")
            } ?: resolver.getType(uri)
        if (mimeType?.startsWith("image/") != true) return@mapNotNull null
        ComposerImageCandidate(uri = uri.toString(), mimeType = mimeType)
    }

private fun candidateFor(resolver: ContentResolver, uri: Uri): ComposerImageCandidate =
    ComposerImageCandidate(uri = uri.toString(), mimeType = resolver.getType(uri))
