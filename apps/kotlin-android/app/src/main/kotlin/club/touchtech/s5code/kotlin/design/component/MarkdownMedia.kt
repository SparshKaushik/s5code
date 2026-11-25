package club.touchtech.s5code.kotlin.design.component

import android.media.AudioAttributes
import android.media.MediaPlayer
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.BrokenImage
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import club.touchtech.s5code.kotlin.design.theme.S5Theme

private enum class AudioState { Idle, Preparing, Playing, Paused, Failed }

/**
 * Counts interruptions so a play request made before one cannot start playback
 * after it, as `useMediaVideoPlayback.createPlayRequest` does on mobile.
 */
internal class PlaybackInterruptions {
    private var count = 0

    fun interrupt() {
        count += 1
    }

    /** True while nothing has interrupted since this request was made. */
    fun request(): () -> Boolean {
        val requestedAt = count
        return { requestedAt == count }
    }
}

/**
 * Inline audio for a Markdown embed, as `ThreadInlineFilePreview` plays one on
 * mobile. The player is created on the first tap and released with the
 * paragraph, so a transcript full of clips holds no decoders while scrolling.
 * Leaving the screen or the app pauses it.
 */
@Composable
internal fun MarkdownAudioEmbed(uri: String, name: String) {
    var state by remember(uri) { mutableStateOf(AudioState.Idle) }
    val player = remember(uri) { arrayOfNulls<MediaPlayer>(1) }
    val interruptions = remember(uri) { PlaybackInterruptions() }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(uri) {
        onDispose {
            player[0]?.release()
            player[0] = null
        }
    }
    DisposableEffect(lifecycle, uri) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_STOP) {
                interruptions.interrupt()
                if (state == AudioState.Playing) {
                    player[0]?.pause()
                    state = AudioState.Paused
                }
            }
        }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }
    fun toggle() {
        val current = player[0]
        when (state) {
            AudioState.Playing -> {
                current?.pause()
                state = AudioState.Paused
            }
            AudioState.Paused -> {
                current?.start()
                state = AudioState.Playing
            }
            AudioState.Preparing -> Unit
            AudioState.Idle, AudioState.Failed -> {
                current?.release()
                state = AudioState.Preparing
                val playIfUninterrupted = interruptions.request()
                player[0] =
                    MediaPlayer().apply {
                        setAudioAttributes(
                            AudioAttributes.Builder()
                                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                                .setUsage(AudioAttributes.USAGE_MEDIA)
                                .build()
                        )
                        setOnPreparedListener {
                            // Coming back must not revive a clip interrupted while it loaded.
                            if (playIfUninterrupted()) {
                                it.start()
                                state = AudioState.Playing
                            } else {
                                state = AudioState.Paused
                            }
                        }
                        setOnCompletionListener {
                            it.seekTo(0)
                            state = AudioState.Paused
                        }
                        setOnErrorListener { _, _, _ ->
                            state = AudioState.Failed
                            true
                        }
                        try {
                            setDataSource(uri)
                            prepareAsync()
                        } catch (_: java.io.IOException) {
                            state = AudioState.Failed
                        } catch (_: IllegalArgumentException) {
                            state = AudioState.Failed
                        } catch (_: SecurityException) {
                            state = AudioState.Failed
                        }
                    }
            }
        }
    }
    MarkdownEmbedChip(
        label = name,
        detail = if (state == AudioState.Failed) "Audio unavailable" else null,
        onClick = ::toggle,
        leading = {
            when (state) {
                AudioState.Preparing -> S5InlineLoading(Modifier.size(20.dp))
                AudioState.Playing -> Icon(Icons.Rounded.Pause, contentDescription = "Pause")
                else -> Icon(Icons.Rounded.PlayArrow, contentDescription = "Play")
            }
        },
    )
}

/** A file an embed names but cannot draw inline; tapping opens it elsewhere. */
@Composable
internal fun MarkdownFileEmbed(name: String, path: String?, onClick: () -> Unit) {
    MarkdownEmbedChip(
        label = name,
        onClick = onClick,
        leading = {
            if (path != null) S5FileIcon(path = path, size = 20.dp)
            else Icon(Icons.AutoMirrored.Rounded.OpenInNew, contentDescription = null, Modifier.size(20.dp))
        },
    )
}

/** `ChatMarkdownImageFallback`: what the embed was, without pretending it loaded. */
@Composable
internal fun MarkdownUnavailableEmbed(alt: String) {
    MarkdownEmbedChip(
        label = listOf("Image unavailable", alt).filter(String::isNotBlank).joinToString(" · "),
        onClick = null,
        leading = { Icon(Icons.Rounded.BrokenImage, contentDescription = null, Modifier.size(20.dp)) },
    )
}

@Composable
private fun MarkdownEmbedChip(
    label: String,
    onClick: (() -> Unit)?,
    leading: @Composable () -> Unit,
    detail: String? = null,
) {
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
    ) {
        Row(
            Modifier.then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
                .heightIn(min = 44.dp)
                .padding(horizontal = S5Theme.spacing.medium, vertical = S5Theme.spacing.small),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) { leading() }
            Text(
                label,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false),
            )
            if (detail != null) {
                Text(detail, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.error)
            }
        }
    }
}
