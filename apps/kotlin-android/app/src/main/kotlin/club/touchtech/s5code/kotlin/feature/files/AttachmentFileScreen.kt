package club.touchtech.s5code.kotlin.feature.files

import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.InsertDriveFile
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.FileProvider
import coil3.compose.AsyncImage
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.Remote
import club.touchtech.s5code.kotlin.data.rememberRetryableRemote
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5CodeBlock
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5Markdown
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.EnvironmentId
import java.io.File
import kotlinx.coroutines.launch

/**
 * The one viewer every sent or staged attachment opens in, matching RN's
 * `AttachmentFileScreen` (`threads/:env/:thread/attachments/:id`).
 *
 * The route params are the whole record — there is no attachment metadata RPC —
 * so the screen mints a signed URL with `disposition: "inline"` and classifies
 * by name and mime. Images render in-app with pinch zoom; text and markdown
 * read a bounded 1 MB prefix; everything else hands the signed URL to an
 * external viewer, falling back to share when no app accepts it.
 */
@Composable
fun AttachmentFileScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    attachmentId: String,
    name: String,
    mimeType: String,
    sizeBytes: Long,
    onBack: () -> Unit,
) {
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val copy = rememberClipboardWriter()
    val kind = remember(name, mimeType) { attachmentPreviewKind(name, mimeType) }
    val displayName = name.ifBlank { "Attachment" }

    val (urlState, retryUrl) =
        rememberRetryableRemote(environmentId, attachmentId) {
            store.workspace.attachmentUrl(
                environmentId = env,
                attachmentId = attachmentId,
                fileName = name.takeIf { it.isNotBlank() },
                mimeType = mimeType.takeIf { it.isNotBlank() },
                disposition = "inline",
            )
        }
    val signedUrl = urlState.value.valueOrNull
    var viewError by remember(attachmentId) { mutableStateOf<String?>(null) }
    var sharing by remember(attachmentId) { mutableStateOf(false) }

    fun openInViewer() {
        val url = signedUrl ?: return
        val intent =
            Intent(Intent.ACTION_VIEW).apply {
                if (mimeType.isNotBlank()) setDataAndType(android.net.Uri.parse(url), mimeType)
                else data = android.net.Uri.parse(url)
            }
        runCatching { context.startActivity(intent) }
            .onFailure { viewError = "No app can open this file." }
    }

    fun share() {
        val url = signedUrl ?: return
        if (sharing) return
        sharing = true
        scope.launch {
            try {
                // Another app cannot use the expiring signed link, so the bytes
                // are downloaded to the shared-attachment cache and handed over
                // as a FileProvider URI.
                val safeName = name.ifBlank { "attachment" }
                    .replace(Regex("[^A-Za-z0-9._-]"), "_")
                    .takeLast(120)
                val target = File(File(context.cacheDir, "shared-attachments"), safeName)
                store.workspace.downloadAsset(url, target)
                val uri =
                    FileProvider.getUriForFile(
                        context,
                        "${context.packageName}.fileprovider",
                        target,
                    )
                val send =
                    Intent(Intent.ACTION_SEND).apply {
                        type = mimeType.ifBlank { "application/octet-stream" }
                        putExtra(Intent.EXTRA_STREAM, uri)
                        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    }
                context.startActivity(Intent.createChooser(send, null))
            } catch (cancelled: kotlinx.coroutines.CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                store.showError(error.message ?: "The file could not be shared.")
            } finally {
                sharing = false
            }
        }
    }

    S5Screen(
        title = displayName,
        subtitle = "Attachment · ${attachmentSizeLabel(sizeBytes)}",
        prominence = S5TopBarProminence.Compact,
        onBack = onBack,
        loading = urlState.value is Remote.Loading || sharing,
        actions = {
            if (signedUrl != null) {
                S5IconButton(
                    icon = Icons.Rounded.Share,
                    label = "Save or share",
                    onClick = ::share,
                )
            }
            // Kinds this screen does not render get the system viewer as their
            // primary presentation, and the same escape stays available for
            // kinds it does.
            if (signedUrl != null && kind != AttachmentPreviewKind.Image) {
                S5IconButton(
                    icon = Icons.AutoMirrored.Rounded.OpenInNew,
                    label = "Open in file viewer",
                    onClick = ::openInViewer,
                )
            }
        },
    ) { padding ->
        when (val url = urlState.value) {
            is Remote.Loading -> S5LoadingState("Preparing the file…", Modifier.padding(padding))
            is Remote.Failed ->
                Box(Modifier.padding(padding).padding(S5Theme.spacing.gutter)) {
                    S5ErrorState(
                        title = "File unavailable",
                        detail = url.message,
                        onRetry = retryUrl,
                    )
                }
            is Remote.Loaded ->
                AttachmentBody(
                    kind = kind,
                    url = url.value,
                    name = name,
                    readText = { store.workspace.readAssetBytes(url.value, ATTACHMENT_TEXT_PREVIEW_MAX_BYTES.toLong()) },
                    viewError = viewError,
                    onOpenViewer = ::openInViewer,
                    onCopy = copy,
                    modifier = Modifier.padding(padding),
                )
        }
    }
}

@Composable
private fun AttachmentBody(
    kind: AttachmentPreviewKind,
    url: String,
    name: String,
    readText: suspend () -> club.touchtech.s5code.kotlin.data.AssetBytesRead,
    viewError: String?,
    onOpenViewer: () -> Unit,
    onCopy: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    when (kind) {
        AttachmentPreviewKind.Image -> ZoomableAttachmentImage(url, name, modifier)
        AttachmentPreviewKind.Markdown,
        AttachmentPreviewKind.Text ->
            AttachmentTextBody(
                url = url,
                name = name,
                markdown = kind == AttachmentPreviewKind.Markdown,
                readText = readText,
                onCopy = onCopy,
                modifier = modifier,
            )
        else ->
            AttachmentNativeFallback(
                viewError = viewError,
                onOpenViewer = onOpenViewer,
                modifier = modifier,
            )
    }
}

/** Pinch/double-tap zoom, the same gestures the workspace image preview uses. */
@Composable
private fun ZoomableAttachmentImage(url: String, name: String, modifier: Modifier = Modifier) {
    var scale by remember(url) { mutableFloatStateOf(1f) }
    var offset by remember(url) { mutableStateOf(Offset.Zero) }
    Box(
        modifier =
            modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.surfaceContainerLowest)
                .pointerInput(url) {
                    detectTransformGestures { _, pan, zoom, _ ->
                        scale = (scale * zoom).coerceIn(1f, 8f)
                        // Panning only means something while zoomed; at 1x the
                        // image is fully visible and a drag would slide it off.
                        offset = if (scale > 1f) offset + pan else Offset.Zero
                    }
                }
                .pointerInput(url) {
                    detectTapGestures(
                        onDoubleTap = {
                            scale = if (scale > 1f) 1f else 2f
                            offset = Offset.Zero
                        }
                    )
                },
        contentAlignment = Alignment.Center,
    ) {
        AsyncImage(
            model = url,
            contentDescription = name,
            contentScale = ContentScale.Fit,
            modifier =
                Modifier.fillMaxSize()
                    .graphicsLayer(
                        scaleX = scale,
                        scaleY = scale,
                        translationX = offset.x,
                        translationY = offset.y,
                    ),
        )
    }
}

@Composable
private fun AttachmentTextBody(
    url: String,
    name: String,
    markdown: Boolean,
    readText: suspend () -> club.touchtech.s5code.kotlin.data.AssetBytesRead,
    onCopy: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val (state, retry) =
        rememberRetryableRemote(url) {
            val read = readText()
            decodeAttachmentPreviewText(read.bytes, read.truncated)
        }
    when (val content = state.value) {
        is Remote.Loading -> S5LoadingState("Reading the file…", modifier)
        is Remote.Failed ->
            Box(modifier.padding(S5Theme.spacing.gutter)) {
                S5ErrorState(
                    title = "Couldn't read this file",
                    detail = content.message,
                    onRetry = retry,
                )
            }
        is Remote.Loaded -> {
            Column(modifier.fillMaxSize()) {
                if (content.value.truncated) {
                    club.touchtech.s5code.kotlin.design.component.S5Notice(
                        icon = Icons.AutoMirrored.Rounded.InsertDriveFile,
                        text =
                            "Preview limited to the first ${attachmentSizeLabel(ATTACHMENT_TEXT_PREVIEW_MAX_BYTES.toLong())}. Share the file to read it in full.",
                        containerColor = MaterialTheme.colorScheme.tertiaryContainer,
                        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                        modifier =
                            Modifier.fillMaxWidth()
                                .padding(horizontal = S5Theme.spacing.gutter),
                    )
                }
                if (markdown) {
                    Box(
                        Modifier.fillMaxSize()
                            .verticalScroll(rememberScrollState())
                            .padding(S5Theme.spacing.gutter)
                    ) {
                        S5Markdown(source = content.value.text, onCopyCode = onCopy)
                    }
                } else {
                    Box(Modifier.fillMaxSize().padding(S5Theme.spacing.gutter)) {
                        S5CodeBlock(
                            lines = content.value.text.lines(),
                            language = name.substringAfterLast('.', ""),
                            onCopy = { onCopy(content.value.text) },
                            wrap = true,
                        )
                    }
                }
            }
        }
    }
}

/**
 * Formats the screen cannot render itself (video, audio, PDF, HTML, unknown).
 * The system viewer is the primary presentation; when nothing accepts it, the
 * honest answer is an empty state and the share action above.
 */
@Composable
private fun AttachmentNativeFallback(
    viewError: String?,
    onOpenViewer: () -> Unit,
    modifier: Modifier = Modifier,
) {
    if (viewError != null) {
        Box(modifier.padding(S5Theme.spacing.gutter)) {
            S5EmptyState(
                icon = Icons.AutoMirrored.Rounded.InsertDriveFile,
                title = "No preview for this file",
                detail = viewError,
            )
        }
        return
    }
    Column(
        modifier.fillMaxSize().padding(S5Theme.spacing.gutter),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = androidx.compose.foundation.layout.Arrangement.Center,
    ) {
        S5EmptyState(
            icon = Icons.AutoMirrored.Rounded.InsertDriveFile,
            title = "Open in file viewer",
            detail = "This file opens in another app on this device.",
        )
        S5Button(
            text = "Open in file viewer",
            onClick = onOpenViewer,
            emphasis = S5ActionEmphasis.Prominent,
            icon = Icons.AutoMirrored.Rounded.OpenInNew,
        )
    }
}
