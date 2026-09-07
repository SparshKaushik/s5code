package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.design.theme.S5ThemeMode
import club.touchtech.s5code.kotlin.model.ApprovalPolicy
import club.touchtech.s5code.kotlin.model.ProjectGrouping
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.model.ThreadSort
import club.touchtech.s5code.kotlin.model.WorkspaceMode
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Persisted-state mapping.
 *
 * The property that matters is tolerance: a build that adds or removes an enum
 * entry must still read an older file, because the alternative is wiping a user's
 * appearance settings and a half-written draft on update.
 */
class ClientStateMappingTest {

    @Test
    fun `preferences round-trip by name`() {
        val runtime =
            RuntimePreferences(
                themeMode = S5ThemeMode.Dark,
                dynamicColor = false,
                projectGrouping = ProjectGrouping.ByRepository,
                threadSort = ThreadSort.Alphabetical,
                snoozedThreadsExpanded = true,
                textScale = 1.2f,
                wrapCode = true,
                terminalTheme = TerminalThemePreference.Dark,
                liveUpdatesEnabled = false,
            )
        assertEquals(runtime, runtime.toStored().toRuntime())
        assertEquals("Dark", runtime.toStored().themeMode)
    }

    @Test
    fun `unknown persisted names fall back to defaults`() {
        val stored =
            StoredPreferences(
                themeMode = "Sepia",
                projectGrouping = "ByPhaseOfMoon",
                threadSort = "Random",
                terminalTheme = "Solarized",
            )
        val runtime = stored.toRuntime()
        assertEquals(S5ThemeMode.System, runtime.themeMode)
        assertEquals(ProjectGrouping.ByProject, runtime.projectGrouping)
        assertEquals(ThreadSort.Recent, runtime.threadSort)
        assertEquals(TerminalThemePreference.App, runtime.terminalTheme)
    }

    @Test
    fun `shelf state round-trips and defaults to collapsed`() {
        // Both shelves closed is the shape a fresh install must read, so an added
        // preference cannot quietly open someone's settled history on update.
        val fresh = StoredPreferences().toRuntime()
        assertEquals(false, fresh.snoozedThreadsExpanded)
        assertEquals(false, fresh.settledThreadsExpanded)

        val expanded =
            RuntimePreferences(snoozedThreadsExpanded = true, settledThreadsExpanded = true)
        assertEquals(expanded, expanded.toStored().toRuntime())
    }

    @Test
    fun `thread settings names decode with per-field fallbacks`() {
        assertEquals(RuntimeMode.Plan, storedRuntimeMode("Plan"))
        assertEquals(RuntimeMode.Default, storedRuntimeMode(""))
        assertEquals(ApprovalPolicy.Full, storedApprovalPolicy("Full"))
        assertEquals(ApprovalPolicy.Ask, storedApprovalPolicy("YOLO"))
        assertEquals(WorkspaceMode.NewWorktree, storedWorkspaceMode("NewWorktree"))
        assertEquals(WorkspaceMode.CurrentCheckout, storedWorkspaceMode("Detached"))
    }

    @Test
    fun `provider options round-trip and keep both value types`() {
        val selections =
            listOf(
                ProviderOptionSelection("effort", ProviderOptionValue.Text("max")),
                ProviderOptionSelection("fastMode", ProviderOptionValue.Flag(true)),
            )
        assertEquals(selections, storedProviderOptions(selections.associate { it.toStored() }))
    }

    @Test
    fun `an option value with neither field set is dropped`() {
        // Written by a build that knew a third value type. Guessing one would send
        // the provider something it never advertised.
        assertEquals(
            listOf(ProviderOptionSelection("effort", ProviderOptionValue.Text("low"))),
            storedProviderOptions(
                mapOf(
                    "effort" to StoredOptionValue(text = "low"),
                    "mystery" to StoredOptionValue(),
                    "blank" to StoredOptionValue(text = " "),
                )
            ),
        )
    }

    @Test
    fun `thread settings staged in a draft round-trip without losing provider options`() {
        val settings =
            ThreadSettings(
                provider = ProviderInstance(instanceId = "work-codex", driver = "codex"),
                model = "gpt-5.4",
                runtimeMode = RuntimeMode.Plan,
                approvalPolicy = ApprovalPolicy.Full,
                options =
                    listOf(
                        ProviderOptionSelection("effort", ProviderOptionValue.Text("high")),
                        ProviderOptionSelection("fast", ProviderOptionValue.Flag(true)),
                    ),
            )
        assertEquals(settings, settings.toStoredThreadSettings().toRuntimeThreadSettings())
    }

    @Test
    fun `attachments round-trip so a restored draft can still be sent`() {
        val stored =
            StoredAttachment(
                id = "a-1",
                name = "screenshot.png",
                mimeType = "image/png",
                sizeBytes = 4_096,
                uri = "file:///cache/composer-attachments/a-1.png",
            )
        assertEquals(stored, stored.toRuntime().toStored())
    }

    @Test
    fun `a stored provider instance keeps the id that routes`() {
        assertEquals(
            ProviderInstance(instanceId = "my-codex", driver = "codex"),
            storedProvider("my-codex", "codex"),
        )
        // A driver this build has never heard of is not a fallback case: the id
        // still routes, and the label degrades instead of the selection.
        assertEquals(
            ProviderInstance(instanceId = "pi", driver = "pi"),
            storedProvider("pi", "pi"),
        )
        assertEquals("pi", storedProvider("pi", "pi").label)
    }

    @Test
    fun `drafts written before open providers still point at the same agent`() {
        assertEquals(
            ProviderInstance(instanceId = "claudeAgent", driver = "claudeAgent"),
            storedProvider("Claude", ""),
        )
        assertEquals(
            ProviderInstance(instanceId = "opencode", driver = "opencode"),
            storedProvider("OpenCode", ""),
        )
    }

    @Test
    fun `an empty stored provider falls back to the default instance`() {
        assertEquals(ProviderInstance.Default, storedProvider("", ""))
    }
}
