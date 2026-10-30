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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.InlineTextContent
import androidx.compose.foundation.text.appendInlineContent
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.WrapText
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
import androidx.compose.runtime.staticCompositionLocalOf
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
import club.touchtech.s5code.kotlin.design.text.MarkdownEmbed
import club.touchtech.s5code.kotlin.design.text.MdBlock
import club.touchtech.s5code.kotlin.design.text.MdImage
import club.touchtech.s5code.kotlin.design.text.MdSpan
import club.touchtech.s5code.kotlin.design.text.MarkdownLinkPresentation
import club.touchtech.s5code.kotlin.design.text.MdTableAlignment
import club.touchtech.s5code.kotlin.design.text.classifyMarkdownEmbed
import club.touchtech.s5code.kotlin.design.text.codeLanguageOf
import club.touchtech.s5code.kotlin.design.text.inlineCodeFileLink
import club.touchtech.s5code.kotlin.design.text.markdownWithHtml
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
    /** Receives a `t3-thread://` link's href; null leaves those links inert. */
    onOpenThreadLink: ((String) -> Unit)? = null,
    /**
     * Renders HTML tags as Markdown, as forges write them into pull request
     * bodies and comments. Off for agent text, where a bare `<Component>` is prose.
     */
    html: Boolean = false,
) {
    val blocks = remember(source, html) { parseMarkdown(if (html) markdownWithHtml(source) else source) }
    val links = remember(workspaceRoot, onOpenFile, onOpenThreadLink) {
        MarkdownLinkActions(workspaceRoot, onOpenFile, onOpenThreadLink)
    }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        blocks.forEach { block ->
            MarkdownBlock(block, textStyle, onCopyCode, links)
        }
    }
}

/** What a tapped link may do inside one rendered document. */
private data class MarkdownLinkActions(
    val workspaceRoot: String? = null,
    val onOpenFile: ((String) -> Unit)? = null,
    val onOpenThreadLink: ((String) -> Unit)? = null,
)

@Composable
private fun MarkdownBlock(
    block: MdBlock,
    textStyle: TextStyle,
    onCopyCode: ((String) -> Unit)?,
    links: MarkdownLinkActions,
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
            MarkdownText(block.text, style, links = links)
        }

        is MdBlock.Paragraph ->
            MarkdownParagraph(block.text, block.images, textStyle, links)

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
                        MarkdownBlock(nested, textStyle, onCopyCode, links)
                    }
                }
            }

        is MdBlock.Code -> {
            // Long lines scroll by default; the header's wrap button folds them
            // in place, per block.
            var wrap by remember(block) { mutableStateOf(false) }
            S5CodeBlock(
                lines = block.lines,
                language = block.language,
                onCopy = onCopyCode?.let { copy -> { copy(block.lines.joinToString("\n")) } },
                wrap = wrap,
                onToggleWrap = { wrap = !wrap },
            )
        }

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
                                    // A minimum, not a fixed width: "10." and
                                    // "100." would otherwise clip.
                                    maxLines = 1,
                                    softWrap = false,
                                    modifier = Modifier.widthIn(min = 24.dp).padding(end = 4.dp),
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
                        MarkdownText(item.text, textStyle, links = links)
                    }
                }
            }

        is MdBlock.Table -> MarkdownTable(block, textStyle, links)

        MdBlock.Rule -> HorizontalDivider(Modifier.padding(vertical = S5Theme.spacing.small))
    }
}

@Composable
private fun MarkdownParagraph(
    spans: List<MdSpan>,
    images: List<MdImage>,
    textStyle: TextStyle,
    links: MarkdownLinkActions,
) {
    var preview by remember(images) { mutableStateOf<MdImage?>(null) }
    val uriHandler = LocalUriHandler.current
    val embeds = remember(images, links.workspaceRoot) { images.map { classifyMarkdownEmbed(it.source, links.workspaceRoot) } }
    Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        if (spans.isNotEmpty()) MarkdownText(spans, textStyle, links = links)
        images.forEachIndexed { index, image ->
            when (val embed = embeds[index]) {
                is MarkdownEmbed.Image ->
                    Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                        AsyncImage(
                            model = embed.uri,
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
                is MarkdownEmbed.Audio -> MarkdownAudioEmbed(embed.uri, image.alt.ifBlank { embed.name })
                is MarkdownEmbed.Document ->
                    MarkdownFileEmbed(image.alt.ifBlank { embed.name }, path = null) { uriHandler.openUri(embed.uri) }
                // The file viewer shows images and plays audio itself, so a shared
                // file opens there rather than as a device path Coil cannot read.
                is MarkdownEmbed.WorkspaceFile -> {
                    val open = links.onOpenFile
                    if (open != null) MarkdownFileEmbed(image.alt.ifBlank { embed.name }, embed.path) { open(embed.path) }
                    else MarkdownUnavailableEmbed(image.alt)
                }
                MarkdownEmbed.Unavailable -> MarkdownUnavailableEmbed(image.alt)
            }
        }
    }
    S5ImageLightbox(
        model = preview?.let { (classifyMarkdownEmbed(it.source, null) as? MarkdownEmbed.Image)?.uri },
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
    links: MarkdownLinkActions,
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
                        links = links,
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
                            links = links,
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
    /** Shows a wrap button in the header that flips [wrap]. */
    onToggleWrap: (() -> Unit)? = null,
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
            if (language != null || onCopy != null || onToggleWrap != null) {
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
                    if (onToggleWrap != null) {
                        S5IconButton(
                            icon = Icons.AutoMirrored.Rounded.WrapText,
                            label = if (wrap) "Don't wrap lines" else "Wrap lines",
                            onClick = onToggleWrap,
                        )
                    }
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
    links: MarkdownLinkActions = MarkdownLinkActions(),
    textAlign: TextAlign? = null,
) {
    val content = spans.annotate(links)
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

/**
 * Opens a web link in the app when it can, returning false to leave it to the
 * browser: a change request link opens its PR page, as `openPullRequestLink`
 * does on web. Provided once at the navigation root.
 */
val LocalMarkdownLinkOpener = staticCompositionLocalOf<((String) -> Boolean)?> { null }

/** Turns inline spans into annotated text plus inline Pierre file icons. */
@Composable
private fun List<MdSpan>.annotate(links: MarkdownLinkActions): AnnotatedMarkdown {
    val (workspaceRoot, onOpenFile, onOpenThreadLink) = links
    val uriHandler = LocalUriHandler.current
    val linkOpener = LocalMarkdownLinkOpener.current
    val linkColor = MaterialTheme.colorScheme.primary
    val codeColor = MaterialTheme.colorScheme.tertiary
    val codeFamily = S5Theme.code.code.fontFamily
    return remember(this, linkColor, codeColor, links, uriHandler, linkOpener) {
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
                val presentation =
                    span.link?.let(::resolveMarkdownLinkPresentation)
                        ?: span.text.takeIf { span.code }?.let(::inlineCodeFileLink)
                when (presentation) {
                    is MarkdownLinkPresentation.File -> {
                        // A trailing slash survives normalization so the opener
                        // knows the link names a directory, not a file.
                        val path =
                            resolveWorkspaceRelativeFilePath(workspaceRoot, presentation.path)?.let {
                                if (presentation.path.endsWith('/') || presentation.path.endsWith('\\')) "$it/" else it
                            }
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
                                                    // The chip shows the file's label, not
                                                    // the code it was written as.
                                                    fontFamily = null,
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
                            ) { if (linkOpener?.invoke(presentation.href) != true) uriHandler.openUri(presentation.href) }
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

                    is MarkdownLinkPresentation.Thread -> {
                        if (onOpenThreadLink != null) {
                            withLink(
                                LinkAnnotation.Clickable(
                                    tag = presentation.href,
                                    styles =
                                        TextLinkStyles(
                                            style = style.copy(color = linkColor, textDecoration = TextDecoration.Underline)
                                        ),
                                ) { onOpenThreadLink(presentation.href) }
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
