package club.touchtech.s5code.kotlin.platform.terminal

/** Pure terminal appearance selection, split from Compose so persistence behavior is testable. */
internal fun resolveTerminalTheme(preference: String, appDark: Boolean): TerminalTheme =
    when (preference) {
        "Light" -> TerminalTheme.light()
        "Dark" -> TerminalTheme.dark()
        else -> if (appDark) TerminalTheme.dark() else TerminalTheme.light()
    }
