package club.touchtech.s5code.kotlin.feature.thread

import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.WarningAmber
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.theme.S5Theme

/** `CHATGPT_USAGE_LIMIT_CODE` in `packages/shared/src/usageLimits.ts`. */
internal const val CHATGPT_USAGE_LIMIT_CODE = "subscription_sharing_usage_limit_exceeded"

/** A ChatGPT Business admin caps plan use in other apps; only the admin can raise it. */
internal const val CHATGPT_WORKSPACE_USAGE_LIMIT_CODE =
    "subscription_sharing_workplace_individual_usage_limit_exceeded"

private const val CHATGPT_USAGE_URL = "https://chatgpt.com/#settings/Usage"

/** OpenAI's partner guidelines fix this wording; the user sends it to their admin themselves. */
internal const val CHATGPT_WORKSPACE_ADMIN_REQUEST =
    "I've reached the limit for using my ChatGPT plan in S5 Code. Could you increase our workspace's " +
        "usage limits so I can continue?\n\nGo to admin.openai.com → Workspace → Usage limits. Thank you!"

private enum class NoticeStatus { Copied, OpenFailed }

/**
 * `ChatGptUsageLimitNotice` from the RN thread screen: a failed ChatGPT run
 * whose provider code says the plan's usage limit was reached. A workspace
 * limit can only be raised by its admin, so it offers a copyable request; a
 * personal limit links to ChatGPT's usage settings. Keyed by the caller on the
 * failed run, so a new failure starts without a stale "copied".
 */
@Composable
internal fun ChatGptUsageLimitNotice(
    lastErrorCode: String?,
    onCopy: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val workspaceLimit = lastErrorCode == CHATGPT_WORKSPACE_USAGE_LIMIT_CODE
    if (!workspaceLimit && lastErrorCode != CHATGPT_USAGE_LIMIT_CODE) return
    val context = LocalContext.current
    var status by remember { mutableStateOf<NoticeStatus?>(null) }
    Surface(
        modifier.fillMaxWidth().semantics { liveRegion = LiveRegionMode.Polite },
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
    ) {
        Column(
            Modifier.padding(S5Theme.spacing.medium),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                Icon(
                    Icons.Rounded.WarningAmber,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = MaterialTheme.colorScheme.tertiary,
                )
                Text("ChatGPT usage limit reached", style = MaterialTheme.typography.titleSmall)
            }
            Text(
                when {
                    !workspaceLimit -> "Review your usage settings in ChatGPT."
                    status == NoticeStatus.Copied -> "Request copied. Send it to your workspace admin."
                    else ->
                        "You've reached your workspace's limit for using your ChatGPT plan in other apps. " +
                            "Copy a request to ask your workspace admin for increased usage limits."
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (workspaceLimit) {
                S5Button(
                    text = "Copy request for admin",
                    onClick = {
                        onCopy(CHATGPT_WORKSPACE_ADMIN_REQUEST)
                        status = NoticeStatus.Copied
                    },
                    emphasis = S5ActionEmphasis.Prominent,
                    style = S5ButtonStyle.Filled,
                )
            } else {
                S5Button(
                    text = "Manage usage",
                    onClick = {
                        val opened = runCatching {
                            context.startActivity(Intent(Intent.ACTION_VIEW, CHATGPT_USAGE_URL.toUri()))
                        }.isSuccess
                        status = if (opened) null else NoticeStatus.OpenFailed
                    },
                    emphasis = S5ActionEmphasis.Prominent,
                    style = S5ButtonStyle.Filled,
                )
            }
            if (status == NoticeStatus.OpenFailed) {
                Text(
                    "Could not open ChatGPT. Go to chatgpt.com → Settings → Usage.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }
        }
    }
}
