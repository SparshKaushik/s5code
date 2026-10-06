package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.v2String
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Test

class ScheduledTaskRowsTest {
    @Test
    fun `known triggers survive unsupported and unidentifiable tasks`() {
        val payload = TransportJson.parseToJsonElement("""{"tasks":[
            {"id":"interval","schedule":{"type":"interval"}},
            {"id":"future","schedule":{"type":"file_change"}},
            {"id":"webhook","schedule":{"type":"webhook"}},
            {"id":"daily","schedule":{"type":"fixed_time"}},
            {"id":" ","schedule":{"type":"webhook"}},
            {"schedule":{"type":"webhook"}},
            {"id":"missing-schedule"}
        ]}""").jsonObject
        assertEquals(listOf("interval", "webhook", "daily"), scheduledTaskRows(payload).map { it.v2String("id") })
    }
}
