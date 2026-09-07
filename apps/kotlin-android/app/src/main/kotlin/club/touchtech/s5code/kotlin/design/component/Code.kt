package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import club.touchtech.s5code.kotlin.design.text.CodeLanguage
import club.touchtech.s5code.kotlin.design.text.CodeTokenKind
import club.touchtech.s5code.kotlin.design.text.codeLanguageOf
import club.touchtech.s5code.kotlin.design.text.highlightLine
import club.touchtech.s5code.kotlin.design.text.highlightLines

/** Token colors, derived from the Pierre Shiki palette so code is vibrant in both themes. */
@Immutable
data class S5CodeColors(
    val keyword: Color,
    val function: Color,
    val type: Color,
    val variable: Color,
    val string: Color,
    val number: Color,
    val comment: Color,
    val annotation: Color,
    val punctuation: Color,
)

@Composable
fun codeColors(): S5CodeColors {
    val dark = isSystemInDarkTheme()
    return remember(dark) {
        if (dark) {
            S5CodeColors(
                keyword = Color(0xFFFF678D),
                function = Color(0xFF9D6AFB),
                type = Color(0xFFD568EA),
                variable = Color(0xFFFFA359),
                string = Color(0xFF5ECC71),
                number = Color(0xFF68CDF2),
                comment = Color(0xFF84848A),
                annotation = Color(0xFF9D6AFB),
                punctuation = Color(0xFF8E8E95),
            )
        } else {
            S5CodeColors(
                keyword = Color(0xFFFC2B73),
                function = Color(0xFF7B43F8),
                type = Color(0xFFC635E4),
                variable = Color(0xFFD47628),
                string = Color(0xFF199F43),
                number = Color(0xFF1CA1C7),
                comment = Color(0xFF84848A),
                annotation = Color(0xFF7B43F8),
                punctuation = Color(0xFF79797F),
            )
        }
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
    return remember(lines, language, colors) {
        highlightLines(lines, language).map { highlight(it, colors) }
    }
}

private fun highlight(line: String, language: CodeLanguage, colors: S5CodeColors): AnnotatedString =
    highlight(highlightLine(line, language), colors)

private fun highlight(tokens: List<club.touchtech.s5code.kotlin.design.text.CodeToken>, colors: S5CodeColors): AnnotatedString =
    buildAnnotatedString {
        tokens.forEach { token ->
            val style =
                when (token.kind) {
                    CodeTokenKind.Keyword -> SpanStyle(colors.keyword, fontWeight = FontWeight.SemiBold)
                    CodeTokenKind.Function -> SpanStyle(colors.function)
                    CodeTokenKind.Type -> SpanStyle(colors.type)
                    CodeTokenKind.Variable -> SpanStyle(colors.variable)
                    CodeTokenKind.StringLiteral -> SpanStyle(colors.string)
                    CodeTokenKind.Number -> SpanStyle(colors.number)
                    CodeTokenKind.Comment -> SpanStyle(colors.comment, fontStyle = FontStyle.Italic)
                    CodeTokenKind.Annotation -> SpanStyle(colors.annotation)
                    CodeTokenKind.Punctuation -> SpanStyle(colors.punctuation)
                    CodeTokenKind.Plain -> SpanStyle()
                }
            withStyle(style) { append(token.text) }
        }
    }
