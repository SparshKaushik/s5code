package club.touchtech.s5code.kotlin.platform.terminal

import androidx.annotation.ColorInt

internal data class TerminalTheme(
    @ColorInt val background: Int,
    @ColorInt val foreground: Int,
    @ColorInt val mutedForeground: Int,
    @ColorInt val cursor: Int,
    val palette: IntArray,
) {
    override fun equals(other: Any?): Boolean =
        other is TerminalTheme &&
            background == other.background &&
            foreground == other.foreground &&
            mutedForeground == other.mutedForeground &&
            cursor == other.cursor &&
            palette.contentEquals(other.palette)

    override fun hashCode(): Int {
        var result = background
        result = 31 * result + foreground
        result = 31 * result + mutedForeground
        result = 31 * result + cursor
        result = 31 * result + palette.contentHashCode()
        return result
    }

    companion object {
        fun light() =
            TerminalTheme(
                background = 0xFFF2F2F7.toInt(),
                foreground = 0xFF6C6C71.toInt(),
                mutedForeground = 0xFF8E8E95.toInt(),
                cursor = 0xFF009FFF.toInt(),
                palette = pierrePalette(0xFF1F1F21.toInt()),
            )

        fun dark() =
            TerminalTheme(
                background = 0xFF0A0A0A.toInt(),
                foreground = 0xFFADADB1.toInt(),
                mutedForeground = 0xFF8E8E95.toInt(),
                cursor = 0xFF009FFF.toInt(),
                palette = pierrePalette(0xFF141415.toInt()),
            )

        private fun pierrePalette(black: Int) =
            intArrayOf(
                black,
                0xFFFF2E3F.toInt(),
                0xFF0DBE4E.toInt(),
                0xFFFFCA00.toInt(),
                0xFF009FFF.toInt(),
                0xFFC635E4.toInt(),
                0xFF08C0EF.toInt(),
                0xFFC6C6C8.toInt(),
                black,
                0xFFFF2E3F.toInt(),
                0xFF0DBE4E.toInt(),
                0xFFFFCA00.toInt(),
                0xFF009FFF.toInt(),
                0xFFC635E4.toInt(),
                0xFF08C0EF.toInt(),
                0xFFC6C6C8.toInt(),
            )
    }
}
