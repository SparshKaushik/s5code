package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.InlineTextContent
import androidx.compose.foundation.text.appendInlineContent
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CheckBox
import androidx.compose.material.icons.rounded.CheckBoxOutlineBlank
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.Placeholder
import androidx.compose.ui.text.PlaceholderVerticalAlign
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil3.compose.AsyncImage
import club.touchtech.s5code.kotlin.design.text.MdBlock
import club.touchtech.s5code.kotlin.design.text.MdImage
import club.touchtech.s5code.kotlin.design.text.MdSpan
import club.touchtech.s5code.kotlin.design.text.MarkdownLinkPresentation
import club.touchtech.s5code.kotlin.design.text.MdTableAlignment
import club.touchtech.s5code.kotlin.design.text.codeLanguageOf
import club.touchtech.s5code.kotlin.design.text.parseMarkdown
import club.touchtech.s5code.kotlin.design.text.resolveMarkdownLinkPresentation
import club.touchtech.s5code.kotlin.design.text.resolveWorkspaceRelativeFilePath
import club.touchtech.s5code.kotlin.design.theme.S5Theme

/**
 * Rich Markdown renderer. Blocks are parsed once per content string, keyed on that
 * string in composition, so a completed transcript entry never re-parses while
 * scrolling. Nothing is retained beyond what is composed — there is no parse cache
 * to grow or bound.
 */
@Composable
fun S5Markdown(
    source: String,
    modifier: Modifier = Modifier,
    textStyle: TextStyle = MaterialTheme.typography.bodyMedium,
    onCopyCode: ((String) -> Unit)? = null,
    workspaceRoot: String? = null,
    onOpenFile: ((String) -> Unit)? = null,
) {
    val blocks = remember(source) { parseMarkdown(source) }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        blocks.forEach { block ->
            MarkdownBlock(block, textStyle, onCopyCode, workspaceRoot, onOpenFile)
        }
    }
}

@Composable
private fun MarkdownBlock(
    block: MdBlock,
    textStyle: TextStyle,
    onCopyCode: ((String) -> Unit)?,
    workspaceRoot: String?,
    onOpenFile: ((String) -> Unit)?,
) {
    when (block) {
        is MdBlock.Heading -> {
            val style =
                when (block.level) {
                    1 -> MaterialTheme.typography.headlineSmallEmphasized
                    2 -> MaterialTheme.typography.titleLargeEmphasized
                    3 -> MaterialTheme.typography.titleMediumEmphasized
                    else -> MaterialTheme.typography.titleSmallEmphasized
                }
            MarkdownText(block.text, style)
        }

        is MdBlock.Paragraph ->
            MarkdownParagraph(block.text, block.images, textStyle)

        is MdBlock.Quote ->
            Surface(
                shape = MaterialTheme.shapes.medium,
                color = MaterialTheme.colorScheme.surfaceContainerHigh,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(
                    Modifier.padding(S5Theme.spacing.medium),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    block.blocks.forEach { nested ->
                        MarkdownBlock(nested, textStyle, onCopyCode, workspaceRoot, onOpenFile)
                    }
                }
            }

        is MdBlock.Code ->
            S5CodeBlock(
                lines = block.lines,
                language = block.language,
                onCopy = onCopyCode?.let { copy -> { copy(block.lines.joinToString("\n")) } },
            )

        is MdBlock.Bullets ->
            Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                block.items.forEach { item ->
                    Row(verticalAlignment = Alignment.Top) {
                        when (item.checked) {
                            null ->
                                Text(
                                    item.marker ?: "•",
                                    style = textStyle,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.width(24.dp),
                                )
                            true ->
                                Icon(
                                    Icons.Rounded.CheckBox,
                                    contentDescription = "Done",
                                    modifier = Modifier.width(24.dp).size(18.dp),
                                    tint = MaterialTheme.colorScheme.primary,
                                )
                            false ->
                                Icon(
                                    Icons.Rounded.CheckBoxOutlineBlank,
                                    contentDescription = "Not done",
                                    modifier = Modifier.width(24.dp).size(18.dp),
                                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                        }
                        MarkdownText(item.text, textStyle)
                    }
                }
            }

        is MdBlock.Table -> MarkdownTable(block, textStyle, workspaceRoot, onOpenFile)

        MdBlock.Rule -> HorizontalDivider(Modifier.padding(vertical = S5Theme.spacing.small))
    }
}

@Composable
private fun MarkdownParagraph(
    spans: List<MdSpan>,
    images: List<MdImage>,
    textStyle: TextStyle,
) {
    var preview by remember(images) { mutableStateOf<MdImage?>(null) }
    Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        if (spans.isNotEmpty()) MarkdownText(spans, textStyle)
        images.forEach { image ->
            Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                AsyncImage(
                    model = image.source,
                    contentDescription = image.alt.ifBlank { null },
                    contentScale = ContentScale.Fit,
                    modifier =
                        Modifier.fillMaxWidth()
                            .clickable { preview = image },
                )
                if (image.alt.isNotBlank()) {
                    Text(
                        image.alt,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
    S5ImageLightbox(
        model = preview?.source,
        contentDescription = preview?.alt,
        imageKey = preview?.source,
        onDismiss = { preview = null },
    )
}

/**
 * Tables scroll horizontally rather than wrapping cells: agent output regularly
 * includes wide comparison tables, and wrapping makes them unreadable.
 */
@Composable
private fun MarkdownTable(
    table: MdBlock.Table,
    textStyle: TextStyle,
    workspaceRoot: String?,
    onOpenFile: ((String) -> Unit)?,
) {
    val scroll = rememberScrollState()
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.horizontalScroll(scroll).padding(S5Theme.spacing.medium)) {
            Row(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.large)) {
                table.header.forEachIndexed { index, cell ->
                    MarkdownText(
                        spans = cell,
                        style = MaterialTheme.typography.labelLargeEmphasized,
                        modifier = Modifier.width(160.dp),
                        workspaceRoot = workspaceRoot,
                        onOpenFile = onOpenFile,
                        textAlign = textAlign(table.alignments.getOrElse(index) { MdTableAlignment.Start }),
                    )
                }
            }
            HorizontalDivider(Modifier.padding(vertical = S5Theme.spacing.small))
            table.rows.forEach { row ->
                Row(
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.large),
                    modifier = Modifier.padding(vertical = S5Theme.spacing.tiny),
                ) {
                    row.forEachIndexed { index, cell ->
                        MarkdownText(
                            spans = cell,
                            style = textStyle,
                            modifier = Modifier.width(160.dp),
                            workspaceRoot = workspaceRoot,
                            onOpenFile = onOpenFile,
                            textAlign = textAlign(table.alignments.getOrElse(index) { MdTableAlignment.Start }),
                        )
                    }
                }
            }
        }
    }
}

/** Copyable, horizontally scrollable fenced code block with syntax colors. */
@Composable
fun S5CodeBlock(
    lines: List<String>,
    modifier: Modifier = Modifier,
    language: String? = null,
    onCopy: (() -> Unit)? = null,
    wrap: Boolean = false,
) {
    val scroll = rememberScrollState()
    val resolvedLanguage = remember(language) { codeLanguageOf(language) }
    val highlightedLines = rememberHighlightedLines(lines, resolvedLanguage)
    Surface(
        modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surfaceContainerHighest,
    ) {
        Column(Modifier.padding(S5Theme.spacing.small)) {
            if (language != null || onCopy != null) {
                Row(
                    Modifier.fillMaxWidth().padding(bottom = S5Theme.spacing.tiny),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        language ?: "text",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(start = S5Theme.spacing.small),
                    )
                    Box(Modifier.weight(1f))
                    if (onCopy != null) {
                        S5IconButton(
                            icon = Icons.Rounded.ContentCopy,
                            label = "Copy code",
                            onClick = onCopy,
                        )
                    }
                }
            }
            SelectionContainer {
                Column(
                    if (wrap) Modifier.fillMaxWidth() else Modifier.horizontalScroll(scroll)
                ) {
                    highlightedLines.forEach { line -> S5CodeLine(line, wrap) }
                }
            }
        }
    }
}

@Composable
private fun S5CodeLine(line: AnnotatedString, wrap: Boolean) {
    Text(
        line,
        style = S5Theme.code.code,
        softWrap = wrap,
        modifier = Modifier.padding(horizontal = S5Theme.spacing.small),
    )
}

/** Rich inline text with Pierre file icons and safe, type-aware link actions. */
@Composable
private fun MarkdownText(
    spans: List<MdSpan>,
    style: TextStyle,
    modifier: Modifier = Modifier,
    workspaceRoot: String? = null,
    onOpenFile: ((String) -> Unit)? = null,
    textAlign: TextAlign? = null,
) {
    val content = spans.annotate(workspaceRoot, onOpenFile)
    SelectionContainer {
        Text(
            content.text,
            style = style,
            modifier = modifier,
            inlineContent = content.inlineContent,
            textAlign = textAlign,
        )
    }
}

private fun textAlign(alignment: MdTableAlignment): TextAlign =
    when (alignment) {
        MdTableAlignment.Start -> TextAlign.Start
        MdTableAlignment.Center -> TextAlign.Center
        MdTableAlignment.End -> TextAlign.End
    }

private data class AnnotatedMarkdown(
    val text: AnnotatedString,
    val inlineContent: Map<String, InlineTextContent>,
)

/** Turns inline spans into annotated text plus inline Pierre file icons. */
@Composable
private fun List<MdSpan>.annotate(
    workspaceRoot: String?,
    onOpenFile: ((String) -> Unit)?,
): AnnotatedMarkdown {
    val uriHandler = LocalUriHandler.current
    val linkColor = MaterialTheme.colorScheme.primary
    val codeColor = MaterialTheme.colorScheme.tertiary
    val codeFamily = S5Theme.code.code.fontFamily
    return remember(this, linkColor, codeColor, workspaceRoot, onOpenFile, uriHandler) {
        val icons = linkedMapOf<String, InlineTextContent>()
        val annotated = buildAnnotatedString {
            this@annotate.forEachIndexed { index, span ->
                val style =
                    SpanStyle(
                        fontWeight = if (span.bold) FontWeight.SemiBold else null,
                        fontStyle = if (span.italic) FontStyle.Italic else null,
                        fontFamily = if (span.code) codeFamily else null,
                        color = if (span.code) codeColor else Color.Unspecified,
                        textDecoration = if (span.strike) TextDecoration.LineThrough else null,
                    )
                val presentation = span.link?.let(::resolveMarkdownLinkPresentation)
                when (presentation) {
                    is MarkdownLinkPresentation.File -> {
                        val path = resolveWorkspaceRelativeFilePath(workspaceRoot, presentation.path)
                        if (path != null && onOpenFile != null) {
                            val iconId = "file-icon-$index"
                            icons[iconId] =
                                InlineTextContent(
                                    Placeholder(16.sp, 16.sp, PlaceholderVerticalAlign.TextCenter)
                                ) {
                                    S5FileIcon(path = presentation.path, size = 16.dp)
                                }
                            withLink(
                                LinkAnnotation.Clickable(
                                    tag = path,
                                    styles =
                                        TextLinkStyles(
                                            style =
                                                style.copy(
                                                    color = linkColor,
                                                    fontWeight = FontWeight.SemiBold,
                                                )
                                        ),
                                ) { onOpenFile(path) }
                            ) {
                                appendInlineContent(iconId, "file")
                                append(' ')
                                append(presentation.label)
                            }
                        } else {
                            withStyle(style) { append(span.text) }
                        }
                    }

                    is MarkdownLinkPresentation.External -> {
                        withLink(
                            LinkAnnotation.Url(
                                presentation.href,
                                TextLinkStyles(
                                    style = style.copy(color = linkColor, textDecoration = TextDecoration.Underline)
                                ),
                            ) { uriHandler.openUri(presentation.href) }
                        ) { append(span.text) }
                    }

                    is MarkdownLinkPresentation.Link -> {
                        val href = presentation.href
                        if (href != null) {
                            withLink(
                                LinkAnnotation.Url(
                                    href,
                                    TextLinkStyles(
                                        style = style.copy(color = linkColor, textDecoration = TextDecoration.Underline)
                                    ),
                                ) { uriHandler.openUri(href) }
                            ) { append(span.text) }
                        } else {
                            withStyle(style) { append(span.text) }
                        }
                    }

                    null -> withStyle(style) { append(span.text) }
                }
            }
        }
        AnnotatedMarkdown(annotated, icons)
    }
}
