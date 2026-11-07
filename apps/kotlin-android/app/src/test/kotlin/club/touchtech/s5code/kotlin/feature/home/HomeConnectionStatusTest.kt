package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.Environment
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.EnvironmentKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class HomeConnectionStatusTest {

    private fun environment(id: String, state: ConnectionState, loaded: Boolean = true) =
        Environment(
            id = EnvironmentId(id),
            label = id,
            host = id,
            kind = EnvironmentKind.Direct,
            state = state,
            snapshotLoaded = loaded,
        )

    private fun status(vararg environments: Environment) =
        homeConnectionStatus(environments.toList(), paired = true, sessionRestored = true)

    @Test
    fun `a sleeping secondary environment keeps the brand`() {
        val primary = environment("desktop", ConnectionState.Connected)
        listOf(ConnectionState.Connecting, ConnectionState.Recovering, ConnectionState.Offline).forEach { state ->
            assertNull(status(primary, environment("laptop", state, loaded = false)))
        }
    }

    @Test
    fun `a connected environment still reading reports sync even beside an unreachable one`() {
        assertEquals(
            "Syncing threads…",
            status(
                environment("desktop", ConnectionState.Connected),
                environment("devbox", ConnectionState.Connected, loaded = false),
                environment("laptop", ConnectionState.Recovering),
            ),
        )
        assertEquals("Loading threads…", status(environment("desktop", ConnectionState.Connected, loaded = false)))
    }

    @Test
    fun `without a connected environment the connection story is told`() {
        assertEquals("You are offline", status(environment("laptop", ConnectionState.Offline)))
        assertEquals("Reconnecting to laptop…", status(environment("laptop", ConnectionState.Recovering)))
        assertEquals(
            "Reconnecting 2 environments",
            status(environment("a", ConnectionState.Connecting), environment("b", ConnectionState.Recovering)),
        )
    }
}
