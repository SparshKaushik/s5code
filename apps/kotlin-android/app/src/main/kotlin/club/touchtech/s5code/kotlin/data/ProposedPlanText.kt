package club.touchtech.s5code.kotlin.data

private val PLAN_HEADING = Regex("""^\s{0,3}#{1,6}\s+(.+)$""")
private val PLAN_HEADING_START = Regex("""^\s{0,3}#{1,6}\s+""")

/** `proposedPlanTitle` from `packages/shared/src/proposedPlanText.ts`: the plan's first heading. */
fun proposedPlanTitle(planMarkdown: String): String? =
    planMarkdown.lineSequence()
        .firstNotNullOfOrNull { PLAN_HEADING.find(it)?.groupValues?.get(1)?.trim() }
        ?.takeIf { it.isNotEmpty() }

/**
 * `stripDisplayedPlanMarkdown`: the body under the card's title. A leading heading
 * is the title already shown, and a "Summary" heading right under it says nothing
 * the card does not.
 */
fun stripDisplayedPlanMarkdown(planMarkdown: String): String {
    val lines = planMarkdown.trimEnd().split(Regex("\r?\n"))
    val source =
        if (lines.firstOrNull()?.let { PLAN_HEADING_START.containsMatchIn(it) } == true) {
            lines.drop(1)
        } else {
            lines
        }.toMutableList()
    fun dropBlankLead() {
        while (source.firstOrNull()?.isBlank() == true) source.removeAt(0)
    }
    dropBlankLead()
    val heading = source.firstOrNull()?.let { PLAN_HEADING.find(it) }?.groupValues?.get(1)
    if (heading?.trim()?.lowercase() == "summary") {
        source.removeAt(0)
        dropBlankLead()
    }
    return source.joinToString("\n")
}

/** The bytes a plan saves and copies as: trailing space trimmed, one newline. */
fun proposedPlanFileContents(planMarkdown: String): String = planMarkdown.trimEnd() + "\n"
