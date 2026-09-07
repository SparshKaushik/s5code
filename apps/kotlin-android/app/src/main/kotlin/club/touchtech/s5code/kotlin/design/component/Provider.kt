package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.res.imageResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.R
import club.touchtech.s5code.kotlin.model.ProviderInstance
import kotlin.math.min
import kotlin.math.roundToInt

/** Official provider vector marks, ported from the React Native client. */
@Composable
fun S5ProviderMark(
    provider: ProviderInstance,
    modifier: Modifier = Modifier,
    size: Dp = 20.dp,
    contentDescription: String? = provider.label,
    tint: Color = MaterialTheme.colorScheme.onSurface,
) {
    val kind = remember(provider.driver) { providerMarkKind(provider.driver) }
    val paths = remember(kind) { geometryFor(kind)?.paths?.map(::parsedPath).orEmpty() }
    val antigravityBitmap =
        if (kind == ProviderMarkKind.Antigravity) {
            ImageBitmap.imageResource(R.drawable.provider_antigravity)
        } else {
            null
        }
    val description = contentDescription
    Canvas(
        modifier
            .size(size)
            .then(
                if (description == null) Modifier
                else Modifier.semantics { this.contentDescription = description }
            )
    ) {
        when (kind) {
            ProviderMarkKind.Claude -> drawGeometry(CLAUDE, paths, Color(0xFFD97757))
            ProviderMarkKind.Cursor -> drawGeometry(CURSOR, paths, tint)
            ProviderMarkKind.Grok -> drawGeometry(GROK, paths, tint)
            ProviderMarkKind.Codex -> drawGeometry(CODEX, paths, tint)
            ProviderMarkKind.OpenCode -> drawOpenCodeMark(tint)
            ProviderMarkKind.Pi -> drawPiMark(paths)
            ProviderMarkKind.Antigravity -> {
                if (antigravityBitmap != null) {
                    drawImage(
                        image = antigravityBitmap,
                        dstSize = IntSize(this.size.width.roundToInt(), this.size.height.roundToInt()),
                    )
                } else {
                    drawGenericMark(tint)
                }
            }
            ProviderMarkKind.Generic -> drawGenericMark(tint)
        }
    }
}

private enum class ProviderMarkKind { Claude, Cursor, Grok, OpenCode, Codex, Antigravity, Pi, Generic }

private fun providerMarkKind(driver: String): ProviderMarkKind =
    when (driver.lowercase()) {
        "claudeagent", "claude" -> ProviderMarkKind.Claude
        "cursor" -> ProviderMarkKind.Cursor
        "grok" -> ProviderMarkKind.Grok
        "opencode" -> ProviderMarkKind.OpenCode
        "codex" -> ProviderMarkKind.Codex
        "antigravity" -> ProviderMarkKind.Antigravity
        "pi" -> ProviderMarkKind.Pi
        else -> ProviderMarkKind.Generic
    }

private data class BrandGeometry(
    val viewportWidth: Float,
    val viewportHeight: Float,
    val paths: List<String>,
)

private fun parsedPath(data: String): Path = PathParser().parsePathString(data).toPath()

private fun geometryFor(kind: ProviderMarkKind): BrandGeometry? =
    when (kind) {
        ProviderMarkKind.Claude -> CLAUDE
        ProviderMarkKind.Cursor -> CURSOR
        ProviderMarkKind.Grok -> GROK
        ProviderMarkKind.Codex -> CODEX
        ProviderMarkKind.Pi -> PI
        else -> null
    }

/** Draws an SVG viewBox with meet scaling, preserving the source geometry and aspect ratio. */
private fun DrawScope.drawGeometry(geometry: BrandGeometry, paths: List<Path>, color: Color) {
    val scale = min(size.width / geometry.viewportWidth, size.height / geometry.viewportHeight)
    val left = (size.width - geometry.viewportWidth * scale) / 2f
    val top = (size.height - geometry.viewportHeight * scale) / 2f
    withTransform({
        translate(left, top)
        scale(scale, scale, pivot = Offset.Zero)
    }) {
        paths.forEach { drawPath(it, color) }
    }
}

private fun DrawScope.drawOpenCodeMark(color: Color) {
    // Exact 32×40 RN geometry, fitted into the square provider slot.
    val scale = min(size.width / 32f, size.height / 40f)
    val left = (size.width - 32f * scale) / 2f
    val top = (size.height - 40f * scale) / 2f
    withTransform({
        translate(left, top)
        scale(scale, scale, pivot = Offset.Zero)
    }) {
        drawRect(color.copy(alpha = 0.28f), topLeft = Offset(8f, 16f), size = Size(16f, 16f))
        drawPath(
            parsedPath("M24 8H8V32H24V8ZM32 40H0V0H32V40Z"),
            color,
        )
    }
}

private fun DrawScope.drawPiMark(paths: List<Path>) {
    val scale = min(size.width / 800f, size.height / 800f)
    withTransform({ scale(scale, scale, pivot = Offset.Zero) }) {
        drawRoundRect(Color.Black, size = Size(800f, 800f), cornerRadius = CornerRadius(160f))
        paths.forEach { drawPath(it, Color.White) }
    }
}

private fun DrawScope.drawGenericMark(color: Color) {
    drawCircle(color.copy(alpha = .15f))
    drawCircle(color, radius = size.minDimension * .22f, style = Stroke(size.minDimension * .10f))
    drawCircle(color, radius = size.minDimension * .05f)
}

/** Provider avatar keeps the shared container but uses the real brand mark. */
@Composable
fun S5ProviderAvatar(
    provider: ProviderInstance,
    modifier: Modifier = Modifier,
    size: Dp = 40.dp,
) {
    androidx.compose.material3.Surface(
        modifier = modifier.size(size),
        shape = club.touchtech.s5code.kotlin.design.theme.S5MaterialShapes.avatar(),
        color = MaterialTheme.colorScheme.tertiaryContainer,
        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
    ) {
        Box(contentAlignment = Alignment.Center) {
            S5ProviderMark(provider, size = size * .54f)
        }
    }
}

private val CLAUDE =
    BrandGeometry(
        256f,
        257f,
        listOf(
            "m50.228 170.321 50.357-28.257.843-2.463-.843-1.361h-2.462l-8.426-.518-28.775-.778-24.952-1.037-24.175-1.296-6.092-1.297L0 125.796l.583-3.759 5.12-3.434 7.324.648 16.202 1.101 24.304 1.685 17.629 1.037 26.118 2.722h4.148l.583-1.685-1.426-1.037-1.101-1.037-25.147-17.045-27.22-18.017-14.258-10.37-7.713-5.25-3.888-4.925-1.685-10.758 7-7.713 9.397.649 2.398.648 9.527 7.323 20.35 15.75L94.817 91.9l3.889 3.24 1.555-1.102.195-.777-1.75-2.917-14.453-26.118-15.425-26.572-6.87-11.018-1.814-6.61c-.648-2.723-1.102-4.991-1.102-7.778l7.972-10.823L71.42 0 82.05 1.426l4.472 3.888 6.61 15.101 10.694 23.786 16.591 32.34 4.861 9.592 2.592 8.879.973 2.722h1.685v-1.556l1.36-18.211 2.528-22.36 2.463-28.776.843-8.1 4.018-9.722 7.971-5.25 6.222 2.981 5.12 7.324-.713 4.73-3.046 19.768-5.962 30.98-3.889 20.739h2.268l2.593-2.593 10.499-13.934 17.628-22.036 7.778-8.749 9.073-9.657 5.833-4.601h11.018l8.1 12.055-3.628 12.443-11.342 14.388-9.398 12.184-13.48 18.147-8.426 14.518.778 1.166 2.01-.194 30.46-6.481 16.462-2.982 19.637-3.37 8.88 4.148.971 4.213-3.5 8.62-20.998 5.184-24.628 4.926-36.682 8.685-.454.324.519.648 16.526 1.555 7.065.389h17.304l32.21 2.398 8.426 5.574 5.055 6.805-.843 5.184-12.962 6.611-17.498-4.148-40.83-9.721-14-3.5h-1.944v1.167l11.666 11.406 21.387 19.314 26.767 24.887 1.36 6.157-3.434 4.86-3.63-.518-23.526-17.693-9.073-7.972-20.545-17.304h-1.36v1.814l4.73 6.935 25.017 37.59 1.296 11.536-1.814 3.76-6.481 2.268-7.13-1.297-14.647-20.544-15.1-23.138-12.185-20.739-1.49.843-7.194 77.448-3.37 3.953-7.778 2.981-6.48-4.925-3.436-7.972 3.435-15.749 4.148-20.544 3.37-16.333 3.046-20.285 1.815-6.74-.13-.454-1.49.194-15.295 20.999-23.267 31.433-18.406 19.702-4.407 1.75-7.648-3.954.713-7.064 4.277-6.286 25.47-32.405 15.36-20.092 9.917-11.6-.065-1.686h-.583L44.07 198.125l-12.055 1.555-5.185-4.86.648-7.972 2.463-2.593 20.35-13.999-.064.065Z"
        ),
    )

private val CURSOR =
    BrandGeometry(
        466.73f,
        532.09f,
        listOf(
            "M457.43,125.94L244.42,2.96c-6.84-3.95-15.28-3.95-22.12,0L9.3,125.94c-5.75,3.32-9.3,9.46-9.3,16.11v247.99c0,6.65,3.55,12.79,9.3,16.11l213.01,122.98c6.84,3.95,15.28,3.95,22.12,0l213.01-122.98c5.75-3.32,9.3-9.46,9.3-16.11v-247.99c0-6.65-3.55-12.79-9.3-16.11h-.01ZM444.05,151.99l-205.63,356.16c-1.39,2.4-5.06,1.42-5.06-1.36v-233.21c0-4.66-2.49-8.97-6.53-11.31L24.87,145.67c-2.4-1.39-1.42-5.06,1.36-5.06h411.26c5.84,0,9.49,6.33,6.57,11.39h-.01Z"
        ),
    )

private val GROK =
    BrandGeometry(
        24f,
        24f,
        listOf(
            "M9.26905 15.284L17.2479 9.36086C17.6391 9.07047 18.1981 9.18374 18.3845 9.63478C19.3655 12.0135 18.9272 14.8721 16.9755 16.8349C15.0238 18.7976 12.3082 19.228 9.8261 18.2477L7.1146 19.5102C11.0037 22.1834 15.7263 21.5223 18.6774 18.5525C21.0182 16.1985 21.7432 12.9897 21.0653 10.0961L21.0714 10.1023C20.0884 5.85143 21.3131 4.15233 23.8218 0.677913C23.8812 0.595532 23.9406 0.513151 24 0.428711L20.6987 3.74866V3.73836L9.267 15.2861",
            "M7.62249 16.7237C4.83113 14.0422 5.3124 9.89222 7.69417 7.49905C9.45541 5.72786 12.341 5.00497 14.86 6.06768L17.5653 4.81138C17.0779 4.45714 16.4533 4.07613 15.7365 3.80839C12.4966 2.46764 8.6178 3.13492 5.98413 5.78141C3.45081 8.32904 2.65415 12.2463 4.02219 15.5889C5.04412 18.0871 3.36889 19.8541 1.68137 21.6377C1.08337 22.2699 0.483318 22.9022 0 23.5716L7.62045 16.7257",
        ),
    )

private val CODEX =
    BrandGeometry(
        256f,
        260f,
        listOf(
            "M239.184 106.203a64.716 64.716 0 0 0-5.576-53.103C219.452 28.459 191 15.784 163.213 21.74A65.586 65.586 0 0 0 52.096 45.22a64.716 64.716 0 0 0-43.23 31.36c-14.31 24.602-11.061 55.634 8.033 76.74a64.665 64.665 0 0 0 5.525 53.102c14.174 24.65 42.644 37.324 70.446 31.36a64.72 64.72 0 0 0 48.754 21.744c28.481.025 53.714-18.361 62.414-45.481a64.767 64.767 0 0 0 43.229-31.36c14.137-24.558 10.875-55.423-8.083-76.483Zm-97.56 136.338a48.397 48.397 0 0 1-31.105-11.255l1.535-.87 51.67-29.825a8.595 8.595 0 0 0 4.247-7.367v-72.85l21.845 12.636c.218.111.37.32.409.563v60.367c-.056 26.818-21.783 48.545-48.601 48.601Zm-104.466-44.61a48.345 48.345 0 0 1-5.781-32.589l1.534.921 51.722 29.826a8.339 8.339 0 0 0 8.441 0l63.181-36.425v25.221a.87.87 0 0 1-.358.665l-52.335 30.184c-23.257 13.398-52.97 5.431-66.404-17.803ZM23.549 85.38a48.499 48.499 0 0 1 25.58-21.333v61.39a8.288 8.288 0 0 0 4.195 7.316l62.874 36.272-21.845 12.636a.819.819 0 0 1-.767 0L41.353 151.53c-23.211-13.454-31.171-43.144-17.804-66.405v.256Zm179.466 41.695-63.08-36.63L161.73 77.86a.819.819 0 0 1 .768 0l52.233 30.184a48.6 48.6 0 0 1-7.316 87.635v-61.391a8.544 8.544 0 0 0-4.4-7.213Zm21.742-32.69-1.535-.922-51.619-30.081a8.39 8.39 0 0 0-8.492 0L99.98 99.808V74.587a.716.716 0 0 1 .307-.665l52.233-30.133a48.652 48.652 0 0 1 72.236 50.391v.205ZM88.061 139.097l-21.845-12.585a.87.87 0 0 1-.41-.614V65.685a48.652 48.652 0 0 1 79.757-37.346l-1.535.87-51.67 29.825a8.595 8.595 0 0 0-4.246 7.367l-.051 72.697Zm11.868-25.58 28.138-16.217 28.188 16.218v32.434l-28.086 16.218-28.188-16.218-.052-32.434Z"
        ),
    )

private val PI =
    BrandGeometry(
        800f,
        800f,
        listOf(
            "M165.29 165.29H517.36V400H400V517.36H282.65V634.72H165.29ZM282.65 282.65V400H400V282.65Z",
            "M517.36 400H634.72V634.72H517.36Z",
        ),
    )
