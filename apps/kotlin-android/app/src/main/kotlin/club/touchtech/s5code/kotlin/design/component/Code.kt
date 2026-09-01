package club.touchtech.s5code.kotlin.design.component

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import club.touchtech.s5code.kotlin.design.text.CodeLanguage
import club.touchtech.s5code.kotlin.design.text.CodeTokenKind
import club.touchtech.s5code.kotlin.design.text.codeLanguageOf
import club.touchtech.s5code.kotlin.design.text.highlightLine
import club.touchtech.s5code.kotlin.design.text.highlightLines
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Token colors, derived from the active scheme so both themes stay readable. */
@Immutable
data class S5CodeColors(
    val keyword: Color,
    val string: Color,
    val number: Color,
    val comment: Color,
    val annotation: Color,
    val punctuation: Color,
)

@Composable
fun codeColors(): S5CodeColors {
    val scheme = MaterialTheme.colorScheme
    return remember(scheme) {
        S5CodeColors(
            keyword = scheme.primary,
            string = scheme.tertiary,
            number = scheme.secondary,
            comment = scheme.onSurfaceVariant,
            annotation = scheme.tertiary,
            punctuation = scheme.onSurfaceVariant,
        )
    }
}

/**
 * Highlights one independent line, keyed on the (line, language) pair in
 * composition. Complete source files, diffs, and fenced blocks use
 * [rememberHighlightedLines] so TextMate state carries across line boundaries.
 */
@Composable
fun rememberHighlighted(line: String, language: String?): AnnotatedString {
    val colors = codeColors()
    val resolved = remember(language) { codeLanguageOf(language) }
    return remember(line, resolved, colors) { highlight(line, resolved, colors) }
}

@Composable
fun rememberHighlighted(line: String, language: CodeLanguage): AnnotatedString {
    val colors = codeColors()
    return remember(line, language, colors) { highlight(line, language, colors) }
}

@Composable
fun rememberHighlightedLines(lines: List<String>, language: String?): List<AnnotatedString> {
    val resolved = remember(language) { codeLanguageOf(language) }
    return rememberHighlightedLines(lines, resolved)
}

@Composable
fun rememberHighlightedLines(lines: List<String>, language: CodeLanguage): List<AnnotatedString> {
    val colors = codeColors()
    val highlighted by
        produceState(
            initialValue = remember(lines) { lines.map(::AnnotatedString) },
            lines,
            language,
            colors,
        ) {
            value =
                withContext(Dispatchers.Default) {
                    highlightLines(lines, language).map { highlight(it, colors) }
                }
        }
    return highlighted
}

private fun highlight(line: String, language: CodeLanguage, colors: S5CodeColors): AnnotatedString =
    highlight(highlightLine(line, language), colors)

private fun highlight(tokens: List<club.touchtech.s5code.kotlin.design.text.CodeToken>, colors: S5CodeColors): AnnotatedString =
    buildAnnotatedString {
        tokens.forEach { token ->
            val style =
                when (token.kind) {
                    CodeTokenKind.Keyword -> SpanStyle(colors.keyword, fontWeight = FontWeight.SemiBold)
                    CodeTokenKind.StringLiteral -> SpanStyle(colors.string)
                    CodeTokenKind.Number -> SpanStyle(colors.number)
                    CodeTokenKind.Comment -> SpanStyle(colors.comment)
                    CodeTokenKind.Annotation -> SpanStyle(colors.annotation)
                    CodeTokenKind.Punctuation -> SpanStyle(colors.punctuation)
                    CodeTokenKind.Plain -> SpanStyle()
                }
            withStyle(style) { append(token.text) }
        }
    }
