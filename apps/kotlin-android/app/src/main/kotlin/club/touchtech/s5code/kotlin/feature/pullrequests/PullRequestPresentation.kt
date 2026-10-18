package club.touchtech.s5code.kotlin.feature.pullrequests

import android.content.Context
import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.CallMerge
import androidx.compose.material.icons.automirrored.rounded.MergeType
import androidx.compose.material.icons.rounded.Block
import androidx.compose.material.icons.rounded.EditNote
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.sp
import androidx.core.net.toUri
import club.touchtech.s5code.kotlin.data.PullRequestStatus
import club.touchtech.s5code.kotlin.data.PullRequestTone
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActorDto
import coil3.compose.AsyncImage

/** The theme color for a [PullRequestTone]. Merged borrows tertiary: the palette has no violet. */
@Composable
fun PullRequestTone.color(): Color =
    when (this) {
        PullRequestTone.Success -> S5Theme.status.settled
        PullRequestTone.Danger -> S5Theme.status.failed
        PullRequestTone.Warning -> S5Theme.status.approval
        PullRequestTone.Muted -> MaterialTheme.colorScheme.onSurfaceVariant
        PullRequestTone.Merged -> MaterialTheme.colorScheme.tertiary
        PullRequestTone.Accent -> MaterialTheme.colorScheme.primary
    }

val PullRequestStatus.icon: ImageVector
    get() =
        when (this) {
            PullRequestStatus.Open -> Icons.AutoMirrored.Rounded.CallMerge
            PullRequestStatus.Merged -> Icons.AutoMirrored.Rounded.MergeType
            PullRequestStatus.Closed -> Icons.Rounded.Block
            PullRequestStatus.Draft -> Icons.Rounded.EditNote
        }

/** A host avatar, or the login's initial on a tonal circle while it loads or when there is none. */
@Composable
fun PullRequestAvatar(actor: PullRequestActorDto?, size: Dp, modifier: Modifier = Modifier) {
    Box(
        modifier.size(size).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceContainerHighest),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            actor?.login?.firstOrNull()?.uppercase() ?: "?",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            fontSize = (size.value * 0.5f).sp,
            style = MaterialTheme.typography.labelSmall,
        )
        val url = actor?.avatarUrl
        if (!url.isNullOrBlank()) {
            AsyncImage(
                model = url,
                contentDescription = actor.login,
                contentScale = ContentScale.Crop,
                modifier = Modifier.size(size),
            )
        }
    }
}

/** Opens [url] in the browser or the host's app; false when nothing can. */
fun openPullRequestUrl(context: Context, url: String): Boolean =
    runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, url.toUri())) }.isSuccess

fun sharePullRequest(context: Context, title: String, url: String): Boolean =
    runCatching {
        val send =
            Intent(Intent.ACTION_SEND).apply {
                type = "text/plain"
                putExtra(Intent.EXTRA_SUBJECT, title)
                putExtra(Intent.EXTRA_TEXT, url)
            }
        context.startActivity(Intent.createChooser(send, null))
    }.isSuccess
