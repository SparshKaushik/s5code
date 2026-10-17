package club.touchtech.s5code.kotlin.transport

import org.junit.Assert.*
import org.junit.Test
import kotlinx.serialization.json.*

class SessionPermissionsTest {
    private fun json(text: String) = TransportJson.parseToJsonElement(text)

    @Test fun `exact permissions override legacy scopes including an empty grant`() {
        val legacy = SessionState(authenticated = true, scopes = listOf("orchestration:operate"))
        assertTrue(legacy.grants("environment:maintain"))
        assertTrue(legacy.grants("settings:write"))
        assertFalse(legacy.copy(permissions = emptyList()).grants("settings:write"))
        assertFalse(legacy.copy(serverUpdateScope = "environment:maintain").grants("settings:write"))
        assertFalse(legacy.copy(authenticated = false).grants("orchestration:operate"))
    }

    @Test fun `operate-only sessions cannot update the server`() {
        val session = SessionState(authenticated = true, permissions = listOf("orchestration:operate"))
        assertThrows(IllegalStateException::class.java) {
            session.requireRpcPermissions("server.updateServer", json("{}"))
        }
        session.copy(permissions = listOf("environment:maintain"))
            .requireRpcPermissions("server.updateServer", json("{}"))
    }

    @Test fun `mixed settings patches require both settings and provider grants`() {
        val session = SessionState(authenticated = true, permissions = listOf("providers:manage"))
        session.requireRpcPermissions("server.updateSettings", json("""{"patch":{"providerInstances":{}}}"""))
        assertThrows(IllegalStateException::class.java) {
            session.requireRpcPermissions("server.updateSettings", json("""{"patch":{"providerInstances":{},"favorites":[]}}"""))
        }
        session.copy(permissions = listOf("providers:manage", "settings:write"))
            .requireRpcPermissions("server.updateSettings", json("""{"patch":{"providerInstances":{},"favorites":[]}}"""))
    }

    @Test fun `reading files or observing a terminal never grants writes`() {
        val session = SessionState(authenticated = true, permissions = listOf("filesystem:read", "terminal:read"))
        session.requireRpcPermissions("projects.readFile", json("{}"))
        assertThrows(IllegalStateException::class.java) { session.requireRpcPermissions("projects.writeFile", json("{}")) }
        assertThrows(IllegalStateException::class.java) { session.requireRpcPermissions("terminal.write", json("{}")) }
    }
}
