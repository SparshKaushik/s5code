package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Test

class RecoveredMessageTextTest {
    @Test
    fun `rejected send preserves a newer draft`() {
        assertEquals("New follow-up\n\nRejected send", recoveredMessageText("New follow-up", "Rejected send"))
        assertEquals("Rejected send", recoveredMessageText("", "Rejected send"))
        assertEquals("Existing draft", recoveredMessageText("Existing draft", ""))
        assertEquals("Rejected send", recoveredMessageText("Rejected send", "Rejected send"))
    }
}
