package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.data.toSnapshot
import club.touchtech.s5code.kotlin.transport.wire.DeviceServiceStateDto
import java.io.File
import java.lang.reflect.Modifier
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The server answers a method it no longer declares with an "Unknown request tag" defect. */
class WsMethodsContractTest {

    private val contracts = File("../../../packages/contracts/src")

    private fun declaredMethodNames(): String =
        listOf("rpc.ts", "orchestrationV2.ts").joinToString("\n") { File(contracts, it).readText() }

    @Test
    fun `every method the client calls is still declared by the contracts`() {
        val declared = declaredMethodNames()
        val methods =
            WsMethods::class.java.declaredFields
                .filter { Modifier.isStatic(it.modifiers) && it.type == String::class.java }
                .map { it.isAccessible = true; it.get(null) as String }
        assertTrue(methods.size > 50)
        val missing = methods.filterNot { declared.contains("\"$it\"") }
        assertEquals(emptyList<String>(), missing)
    }

    @Test
    fun `simlock hosts arrive in their own array and still count as hosts`() {
        val state =
            TransportJson.decodeFromString(
                DeviceServiceStateDto.serializer(),
                """{"hosts":[{"id":"local","kind":"local","label":"This Mac"}],
                   "easHosts":[{"id":"eas","kind":"eas","label":"EAS"}],
                   "simlockHosts":[{"id":"pool","kind":"simlock","label":"Simlock"}]}""",
            )
        assertEquals(listOf("local", "eas", "pool"), state.toSnapshot().hosts.map { it.id })
    }
}
