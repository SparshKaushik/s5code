package club.touchtech.s5code.kotlin.feature.usage

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.graphics.toColorInt
import club.touchtech.s5code.kotlin.data.displayLimitWindows
import club.touchtech.s5code.kotlin.data.formatLimitDuration
import club.touchtech.s5code.kotlin.data.formatResetLabel
import club.touchtech.s5code.kotlin.data.paceRemainingPercent
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.LimitAccount
import club.touchtech.s5code.kotlin.model.LimitPool
import club.touchtech.s5code.kotlin.model.LimitPoolWindow
import club.touchtech.s5code.kotlin.model.UsageLimitsView

/**
 * The pooled subscription-quota view — `UsageLimitsPooled` on RN. One row per
 * window id, the bar split into fixed account columns so a gap reads as a gap.
 * The composer panel reuses this in a tighter card; the Usage tab shows it
 * full-width.
 */

private val DRIVER_LABELS = mapOf("codex" to "Codex", "claudeAgent" to "Claude")

private fun driverLabel(driver: String): String = DRIVER_LABELS[driver] ?: driver

private fun accountName(account: LimitAccount): String {
    account.displayName?.let { return it }
    val email = account.email ?: return driverLabel(account.driver)
    val local = email.substringBefore('@')
    val domain = email.substringAfter('@', "")
    val initials = "${local.firstOrNull() ?: ""}${domain.firstOrNull() ?: ""}".uppercase()
    return initials.ifEmpty { "Account" }
}

private val PACE_LABELS = mapOf("ahead" to "Ahead of pace", "on" to "On pace", "under" to "Under pace")

private fun accountColor(account: LimitAccount, index: Int): Color {
    val accent = account.accentColor?.let { runCatching { Color(it.toColorInt()) }.getOrNull() }
    if (accent != null) return accent
    // Hash the key so a hub account without a tint still lands on a stable color.
    val palette = listOf(0xFF6E56CF, 0xFF0E7490, 0xFFB45309, 0xFF15803D, 0xFFBE185D)
    return Color(palette[(account.key.hashCode() + index).mod(palette.size)])
}

private val PACE_GAP_HALF_WIDTH = 3.dp

/**
 * Where the even-pace tick goes in a bar [width] wide, kept [halfGap] inside
 * either end so the tick and its gap stay on the bar at 0% and 100%. Null when
 * the bar is too narrow to hold a gap at all.
 */
internal fun paceTickX(width: Float, timeLeftPercent: Int, halfGap: Float): Float? {
    if (width < halfGap * 2) return null
    return (width * timeLeftPercent / 100f).coerceIn(halfGap, width - halfGap)
}

@Composable
private fun PoolBar(window: LimitPoolWindow, now: Long) {
    // One weighted column per account, in the pool's fixed order: an account
    // without this window still takes its slot, so gaps read as gaps.
    Row(Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp))) {
        window.columns.forEachIndexed { index, (account, memberWindow) ->
            val remaining = memberWindow?.let { 100.0 - it.usedPercent.coerceIn(0.0, 100.0) }
            val timeLeft = memberWindow?.let { paceRemainingPercent(it, now) }
            val tickColor = MaterialTheme.colorScheme.onSurface
            val trackColor = MaterialTheme.colorScheme.surfaceContainerHighest
            Box(
                Modifier.weight(1f)
                    .fillMaxHeight()
                    .semantics {
                        contentDescription =
                            if (remaining == null) "${accountName(account)}: no limit reported"
                            else
                                "${accountName(account)}: ${remaining.toInt()}% left" +
                                    (timeLeft?.let { ", $it% of the window left" } ?: "")
                    }
                    // Even pace: quota left past this mark is ahead of the clock. The
                    // tick sits in a see-through gap cut from the track and fill,
                    // so it reads against either side without changing their shape.
                    .drawWithContent {
                        val gap = PACE_GAP_HALF_WIDTH.toPx()
                        val tickX = timeLeft?.let { paceTickX(size.width, it, gap) }
                        if (tickX == null) {
                            drawContent()
                        } else {
                            clipRect(right = tickX - gap) { this@drawWithContent.drawContent() }
                            clipRect(left = tickX + gap) { this@drawWithContent.drawContent() }
                            val tick = 2.dp.toPx()
                            val inset = 1.dp.toPx()
                            drawRoundRect(
                                tickColor,
                                topLeft = Offset(tickX - tick / 2, inset),
                                size = Size(tick, size.height - inset * 2),
                                cornerRadius = CornerRadius(tick / 2),
                            )
                        }
                    }
                    .background(trackColor)
            ) {
                Box(
                    Modifier.fillMaxHeight()
                        .fillMaxWidth(((remaining ?: 0.0) / 100).toFloat())
                        .background(accountColor(account, index))
                )
            }
        }
    }
}

@Composable
private fun LimitAccountLine(
    account: LimitAccount,
    now: Long,
    onRedeemCredit: (LimitAccount) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(
                    accountName(account),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                val subtitle =
                    listOfNotNull(
                        account.plan,
                        account.sourceLabel,
                        account.environments.takeIf { it.isNotEmpty() }?.joinToString(", "),
                    ).joinToString(" · ")
                if (subtitle.isNotEmpty()) {
                    Text(
                        subtitle,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        val credits = account.limits.resetCredits
        if (credits != null && credits.availableCount > 0 && account.redeem != null) {
            val expiry =
                credits.nextExpiresAtMillis?.takeIf { it > now }
                    ?.let { "expires in ${formatLimitDuration(it - now)}" }
            TextButton(onClick = { onRedeemCredit(account) }) {
                Text(
                    buildString {
                        append("Reset usage")
                        if (credits.availableCount > 1) append(" (${credits.availableCount} credits)")
                        expiry?.let { append(" · $it") }
                    }
                )
            }
        }
    }
}

@Composable
private fun LimitWindowCard(
    window: LimitPoolWindow,
    now: Long,
) {
    Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                window.label,
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.weight(1f),
            )
            Text(
                "${window.remainingPercent}% left",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        PoolBar(window, now)
        Row(verticalAlignment = Alignment.CenterVertically) {
            val detail =
                listOfNotNull(
                    window.pace?.let { PACE_LABELS[it] },
                    window.resets.firstOrNull { it.restoresPercent > 0 }?.let { reset ->
                        val member = window.members.firstOrNull { it.first.key == reset.accountKey }
                        val when_ = formatResetLabel(reset.atMillis, now)
                        buildString {
                            member?.let { append("${accountName(it.first)} $when_") }
                                ?: append(when_)
                            if (reset.restoresPercent > 0) append(" (+${reset.restoresPercent}%)")
                        }
                    },
                ).joinToString(" · ")
            if (detail.isNotEmpty()) {
                Text(
                    detail,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

/**
 * The pooled limits list. [driver] narrows to one provider for the composer
 * panel; the Usage tab passes null and gets every pool.
 */
@Composable
fun UsageLimitsContent(
    view: UsageLimitsView,
    now: Long,
    onRedeemCredit: (LimitAccount) -> Unit,
    modifier: Modifier = Modifier,
    driver: String? = null,
) {
    val pools =
        remember(view, driver) {
            (if (driver == null) view.pools else view.pools.filter { it.driver == driver })
        }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        if (pools.isEmpty() && view.notices.isEmpty()) {
            Text(
                "No subscription limits reported.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        pools.forEach { pool ->
            Surface(
                shape = MaterialTheme.shapes.large,
                color = MaterialTheme.colorScheme.surfaceContainerLow,
            ) {
                Column(
                    Modifier.padding(S5Theme.spacing.medium),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    Text(
                        driverLabel(pool.driver),
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurface,
                    )
                    displayLimitWindows(pool).forEach { window ->
                        LimitWindowCard(window, now)
                    }
                    pool.accounts.forEach { account ->
                        LimitAccountLine(account, now, onRedeemCredit)
                    }
                }
            }
        }
        view.notices.forEach { notice ->
            Text(
                notice,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = S5Theme.spacing.medium),
            )
        }
    }
}

/**
 * The `/usage-limits` answer, docked above the composer — `ComposerUsageLimits`
 * on RN. Opaque, so nothing blurs the feed behind it.
 */
@Composable
fun ComposerUsageLimitsCard(
    view: UsageLimitsView,
    driver: String,
    now: Long,
    onRedeemCredit: (LimitAccount) -> Unit,
    onClose: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier,
        shape = MaterialTheme.shapes.extraLarge,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
    ) {
        Column {
            Row(
                Modifier.fillMaxWidth()
                    .padding(start = S5Theme.spacing.medium, end = S5Theme.spacing.tiny, top = S5Theme.spacing.tiny),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "Usage limits",
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.weight(1f),
                )
                IconButton(onClick = onClose, modifier = Modifier.size(28.dp)) {
                    Icon(
                        Icons.Rounded.Close,
                        contentDescription = "Dismiss usage limits",
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            // The close row stays put while the limits scroll. The caller gives the
            // card only the height left above the composer, so with the keyboard open
            // the limits shrink instead of pushing the close row under the header.
            val screenHeight = with(LocalDensity.current) { LocalWindowInfo.current.containerSize.height.toDp() }
            UsageLimitsContent(
                view = view,
                now = now,
                driver = driver,
                onRedeemCredit = onRedeemCredit,
                modifier =
                    Modifier.weight(1f, fill = false)
                        .heightIn(max = screenHeight * 0.4f)
                        .verticalScroll(rememberScrollState())
                        .padding(S5Theme.spacing.small),
            )
        }
    }
}

