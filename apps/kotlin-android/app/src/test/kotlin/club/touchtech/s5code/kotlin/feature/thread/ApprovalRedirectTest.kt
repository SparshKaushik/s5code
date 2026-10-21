package club.touchtech.s5code.kotlin.feature.thread

import club.touchtech.s5code.kotlin.app.ThreadDraft
import club.touchtech.s5code.kotlin.model.ApprovalKind
import club.touchtech.s5code.kotlin.model.PendingApproval
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Sending over an open approval declines it and redirects the agent. Getting this
 * wrong either leaves a gate blocking the redirect or sends a draft the user had
 * already changed.
 */
class ApprovalRedirectTest {
    private fun approval(id: String) = PendingApproval(id, "Run command", "", null, ApprovalKind.Command)

    private val first = approval("first")
    private val second = approval("second")

    @Test
    fun `declines every pending approval in order`() = runTest {
        val declined = mutableListOf<String>()
        assertNull(declinePendingApprovals(listOf(first, second)) { declined += it })
        assertEquals(listOf("first", "second"), declined)
    }

    @Test
    fun `stops at the first failed decline`() = runTest {
        val failure = IllegalStateException("decline failed")
        val declined = mutableListOf<String>()
        val result =
            declinePendingApprovals(listOf(first, second)) {
                declined += it
                throw failure
            }
        assertSame(failure, result)
        assertEquals(listOf("first"), declined)
    }

    @Test
    fun `an unchanged draft with only declined approvals still sends`() {
        val draft = ThreadDraft(text = "Use the other approach")
        assertTrue(redirectStillSendable(draft, draft.copy(), listOf(first), listOf(first)))
        assertTrue(redirectStillSendable(draft, draft, listOf(first), emptyList()))
    }

    @Test
    fun `a draft edited during the decline does not send`() {
        val draft = ThreadDraft(text = "Use the other approach")
        assertFalse(
            redirectStillSendable(draft, draft.copy(text = "Use the other approach, and"), listOf(first), listOf(first)),
        )
    }

    @Test
    fun `an approval that arrived during the decline holds the draft`() {
        val draft = ThreadDraft(text = "Stop")
        assertFalse(redirectStillSendable(draft, draft, listOf(first), listOf(first, second)))
    }
}
