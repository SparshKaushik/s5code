package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.wire.ServerConfigSettingsDto
import club.touchtech.s5code.kotlin.transport.wire.ServerProviderDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ProjectProvidersTest {

    private val settings =
        TransportJson.decodeFromString(
            ServerConfigSettingsDto.serializer(),
            """{
                "providerInstances":{
                    "codex":{"driver":"codex","enabled":true},
                    "cursor":{"driver":"cursor"},
                    "claude-work":{"driver":"claudeAgent","enabled":true,"config":{"enabled":false}}
                },
                "projectSettingsOverrides":{
                    "p1":{"providerInstances":{"codex":{"enabled":false},"cursor":{"enabled":true}}},
                    "p2":{}
                }
            }""",
        )

    @Test
    fun `a project's own switch wins over the machine-wide instance`() {
        assertFalse(resolveProjectProviderInstanceEnabled(settings, "p1", "codex"))
        assertTrue(resolveProjectProviderInstanceEnabled(settings, "p2", "codex"))
        assertTrue(resolveProjectProviderInstanceEnabled(settings, "p1", "cursor"))
        assertEquals(mapOf("p1" to false), projectProviderInstanceOverrides(settings, "codex"))
    }

    @Test
    fun `without a project switch the instance config and driver defaults decide`() {
        // Cursor defaults off; either switch off wins; unconfigured built-ins are on.
        assertFalse(resolveProjectProviderInstanceEnabled(settings, null, "cursor"))
        assertFalse(resolveProjectProviderInstanceEnabled(settings, null, "claude-work"))
        assertTrue(resolveProjectProviderInstanceEnabled(settings, null, "claudeAgent"))
        assertFalse(resolveProjectProviderInstanceEnabled(settings, null, "unknown"))
    }

    @Test
    fun `a send is refused only for a known instance that is off for the project`() {
        val providers = listOf(ServerProviderDto(instanceId = "codex"), ServerProviderDto(instanceId = "cursor"))
        assertTrue(isModelTurnedOffForProject(settings, providers, "p1", "codex"))
        assertFalse(isModelTurnedOffForProject(settings, providers, "p2", "codex"))
        assertFalse(isModelTurnedOffForProject(settings, providers, "p1", "cursor"))
        // An instance the server no longer lists is a different error, not this one.
        assertFalse(isModelTurnedOffForProject(settings, providers, "p2", "unknown"))
    }
}
