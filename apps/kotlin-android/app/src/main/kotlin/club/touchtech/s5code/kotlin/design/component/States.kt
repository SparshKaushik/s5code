package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material3.ContainedLoadingIndicator
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.theme.S5MaterialShapes
import club.touchtech.s5code.kotlin.design.theme.S5PillShape
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import java.util.Locale

/**
 * Full-screen empty state. Iconic hero shape + emphasized headline + one
 * large action, which is the expressive treatment for "nothing here yet".
 */
@Composable
fun S5EmptyState(
    icon: ImageVector,
    title: String,
    detail: String,
    modifier: Modifier = Modifier,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
) {
    Column(
        modifier
            .fillMaxSize()
            .padding(horizontal = S5Theme.spacing.xxLarge),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        S5ShapeBadge(
            icon = icon,
            contentDescription = null,
            shape = S5MaterialShapes.hero(),
            containerColor = MaterialTheme.colorScheme.secondaryContainer,
            contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
            size = 96.dp,
            iconSize = 40.dp,
        )
        Text(
            title,
            style = MaterialTheme.typography.headlineSmallEmphasized,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = S5Theme.spacing.xLarge),
        )
        Text(
            detail,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = S5Theme.spacing.small).widthIn(max = 420.dp),
        )
        if (actionLabel != null && onAction != null) {
            Box(Modifier.padding(top = S5Theme.spacing.xLarge)) {
                S5Button(
                    text = actionLabel,
                    onClick = onAction,
                    emphasis = S5ActionEmphasis.Primary,
                )
            }
        }
    }
}

/** Bounded loading moment: one visible expressive indicator, never per-row. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5LoadingState(label: String, modifier: Modifier = Modifier) {
    Column(
        modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        ContainedLoadingIndicator()
        Text(
            label,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = S5Theme.spacing.large),
        )
    }
}

/** Inline indicator for a bounded in-place load (a row expanding, a retry). */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5InlineLoading(modifier: Modifier = Modifier) {
    LoadingIndicator(modifier.size(24.dp))
}

/**
 * Full-screen wait state: what the app is waiting on, with a way forward.
 *
 * The counterpart of `EnvironmentConnectionNotice` in the RN client, and used in the
 * same place — a destination with nothing loaded yet to draw. [spinning] is passed
 * rather than derived so a stalled phase gets a static icon: an indicator that keeps
 * morphing on a dead connection claims progress that is not happening.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5WaitState(
    title: String,
    detail: String,
    icon: ImageVector,
    spinning: Boolean,
    modifier: Modifier = Modifier,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
) {
    Column(
        modifier.fillMaxSize().padding(horizontal = S5Theme.spacing.xxLarge),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        if (spinning) {
            ContainedLoadingIndicator()
        } else {
            S5ShapeBadge(
                icon = icon,
                contentDescription = null,
                shape = S5MaterialShapes.hero(),
                containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                size = 72.dp,
                iconSize = 32.dp,
            )
        }
        Text(
            title,
            style = MaterialTheme.typography.titleMediumEmphasized,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = S5Theme.spacing.large),
        )
        Text(
            detail,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = S5Theme.spacing.small).widthIn(max = 420.dp),
        )
        if (actionLabel != null && onAction != null) {
            Box(Modifier.padding(top = S5Theme.spacing.large)) {
                S5Button(
                    text = actionLabel,
                    onClick = onAction,
                    emphasis = S5ActionEmphasis.Prominent,
                    style = S5ButtonStyle.Tonal,
                )
            }
        }
    }
}

/**
 * The pill form of a wait state, for a screen that already has content: it floats
 * over live data instead of replacing it, exactly as the RN composer's status pill
 * does.
 *
 * Sized for one line, because the pill overlaps content and a growing banner would
 * cover the newest rows — the ones being waited on.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5WaitPill(
    label: String,
    spinning: Boolean,
    modifier: Modifier = Modifier,
    onClick: (() -> Unit)? = null,
) {
    Surface(
        modifier = if (onClick == null) modifier else modifier.clickable(onClick = onClick),
        shape = S5PillShape,
        color = MaterialTheme.colorScheme.surfaceContainerHighest,
        contentColor = MaterialTheme.colorScheme.onSurface,
        tonalElevation = 3.dp,
        shadowElevation = 2.dp,
    ) {
        Row(
            Modifier.padding(horizontal = S5Theme.spacing.medium, vertical = S5Theme.spacing.small),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            if (spinning) {
                LoadingIndicator(Modifier.size(16.dp))
            } else {
                Box(
                    Modifier.size(8.dp).background(MaterialTheme.colorScheme.error, S5PillShape)
                )
            }
            Text(
                label,
                style = MaterialTheme.typography.labelLargeEmphasized,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.widthIn(max = 260.dp),
            )
        }
    }
}

/**
 * Skeleton row placeholder for repeated dense loading. Static by design: a list
 * of morphing indicators would repaint continuously.
 */
@Composable
fun S5SkeletonRow(modifier: Modifier = Modifier) {
    Column(
        modifier
            .fillMaxWidth()
            .padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.medium),
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        Box(
            Modifier
                .fillMaxWidth(0.35f)
                .height(12.dp)
                .background(MaterialTheme.colorScheme.surfaceContainerHigh, S5PillShape)
        )
        Box(
            Modifier
                .fillMaxWidth(0.8f)
                .height(16.dp)
                .background(MaterialTheme.colorScheme.surfaceContainerHigh, S5PillShape)
        )
    }
}

/**
 * Case-insensitive visible-substring highlighting for list search results.
 *
 * All occurrences are bolded and recolored while preserving the source text's
 * original casing. The function is pure so matching edge cases can be unit
 * tested without a Compose rule.
 */
fun highlightedText(
    text: String,
    query: String,
    normalColor: Color,
    highlightColor: Color,
): AnnotatedString {
    val needle = query.trim()
    if (needle.isEmpty()) return AnnotatedString(text)
    val haystack = text.lowercase(Locale.ROOT)
    val foldedNeedle = needle.lowercase(Locale.ROOT)
    return buildAnnotatedString {
        var cursor = 0
        while (cursor < text.length) {
            val index = haystack.indexOf(foldedNeedle, startIndex = cursor)
            if (index < 0) {
                withStyle(SpanStyle(color = normalColor)) { append(text.substring(cursor)) }
                break
            }
            if (index > cursor) {
                withStyle(SpanStyle(color = normalColor)) {
                    append(text.substring(cursor, index))
                }
            }
            withStyle(SpanStyle(color = highlightColor, fontWeight = FontWeight.Bold)) {
                append(text.substring(index, index + foldedNeedle.length))
            }
            cursor = index + foldedNeedle.length
        }
    }
}

/** Error surface with an explicit retry. Every failure has a way forward. */
@Composable
fun S5ErrorState(
    title: String,
    detail: String,
    modifier: Modifier = Modifier,
    onRetry: (() -> Unit)? = null,
    retryLabel: String = "Try again",
) {
    Surface(
        modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.errorContainer,
        contentColor = MaterialTheme.colorScheme.onErrorContainer,
    ) {
        Column(
            Modifier.padding(S5Theme.spacing.large),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Text(title, style = MaterialTheme.typography.titleMediumEmphasized)
            Text(detail, style = MaterialTheme.typography.bodyMedium)
            if (onRetry != null) {
                S5Button(
                    text = retryLabel,
                    onClick = onRetry,
                    emphasis = S5ActionEmphasis.Prominent,
                    style = S5ButtonStyle.Tonal,
                )
            }
        }
    }
}

/**
 * Compact status label. Color always pairs with text, and an optional iconic
 * shape dot carries the same state for quick scanning.
 */
@Composable
fun S5StatusPill(
    label: String,
    containerColor: Color,
    contentColor: Color,
    modifier: Modifier = Modifier,
    shape: Shape = S5PillShape,
) {
    Surface(modifier, shape = shape, color = containerColor, contentColor = contentColor) {
        Row(
            Modifier.padding(horizontal = S5Theme.spacing.small, vertical = S5Theme.spacing.tiny),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(label, style = MaterialTheme.typography.labelSmallEmphasized)
        }
    }
}

/**
 * Muted informational banner (a degraded provider, an offline read, a capability the
 * server does not have).
 *
 * Pass [onDismiss] when the notice reports a one-off failure the user should be
 * able to clear, such as a refused attachment.
 */
@Composable
fun S5Notice(
    icon: ImageVector,
    text: String,
    modifier: Modifier = Modifier,
    containerColor: Color = MaterialTheme.colorScheme.surfaceContainerHigh,
    contentColor: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    onDismiss: (() -> Unit)? = null,
) {
    Surface(
        modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.medium,
        color = containerColor,
        contentColor = contentColor,
    ) {
        Row(
            Modifier.padding(
                start = S5Theme.spacing.medium,
                end = if (onDismiss == null) S5Theme.spacing.medium else S5Theme.spacing.tiny,
                top = S5Theme.spacing.small,
                bottom = S5Theme.spacing.small,
            ),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(text, style = MaterialTheme.typography.bodySmall, modifier = Modifier.weight(1f))
            if (onDismiss != null) {
                S5IconButton(
                    icon = Icons.Rounded.Close,
                    label = "Dismiss",
                    onClick = onDismiss,
                )
            }
        }
    }
}
