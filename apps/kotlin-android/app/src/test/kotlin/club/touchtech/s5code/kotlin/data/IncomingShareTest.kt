package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class IncomingShareTest {

    private fun draft(id: String, createdAt: String = "2026-01-01T00:00:00Z") =
        IncomingShareDraft(id = id, createdAt = createdAt, text = "hello")

    private fun image(name: String = "shot.png", bytes: Long = 1024) =
        IncomingShareAttachment(
            id = "share-x:image:0",
            type = IncomingShareAttachmentType.Image,
            name = name,
            mimeType = "image/png",
            sizeBytes = bytes,
            uri = "file:///inbox/$name",
        )

    private fun file(name: String = "notes.pdf", bytes: Long = 2048) =
        IncomingShareAttachment(
            id = "share-x:file:0",
            type = IncomingShareAttachmentType.File,
            name = name,
            mimeType = "application/pdf",
            sizeBytes = bytes,
            uri = "file:///inbox/$name",
        )

    @Test
    fun `share ids are stable across re-delivery`() {
        assertEquals(
            incomingShareIdFor("hello", "text/plain", emptyList()),
            incomingShareIdFor("hello", "text/plain", emptyList()),
        )
        assertNotEquals(
            incomingShareIdFor("hello", "text/plain", emptyList()),
            incomingShareIdFor("hello", "image/png", listOf("content://x/1")),
        )
    }

    @Test
    fun `drafts sort newest first and dedupe by id`() {
        val sorted =
            sortAndDedupeIncomingShares(
                listOf(
                    draft("a", "2026-01-01T00:00:00Z"),
                    draft("b", "2026-01-02T00:00:00Z"),
                    draft("a", "2026-01-03T00:00:00Z"),
                )
            )
        assertEquals(listOf("a", "b"), sorted.map { it.id })
    }

    @Test
    fun `empty drafts have no content`() {
        assertFalse(hasIncomingShareContent(draft("a").copy(text = "  ")))
        assertTrue(hasIncomingShareContent(draft("a").copy(attachments = listOf(image()))))
        assertTrue(hasIncomingShareContent(draft("a")))
    }

    @Test
    fun `a server without file support drops file attachments with a warning`() {
        val (kept, warnings) =
            selectIncomingShareAttachments(listOf(image(), file()), maxFileBytes = null)
        assertEquals(1, kept.size)
        assertEquals(IncomingShareAttachmentType.Image, kept.single().type)
        assertTrue(warnings.single().contains("does not support files"))
    }

    @Test
    fun `files over the server ceiling are refused`() {
        val (kept, warnings) =
            selectIncomingShareAttachments(
                listOf(file(bytes = 80L * 1024 * 1024)),
                maxFileBytes = 50L * 1024 * 1024,
            )
        assertTrue(kept.isEmpty())
        assertTrue(warnings.single().contains("50 MB"))
    }

    @Test
    fun `files within the server ceiling are kept`() {
        val (kept, warnings) =
            selectIncomingShareAttachments(
                listOf(image(), file()),
                maxFileBytes = 10L * 1024 * 1024,
            )
        assertEquals(2, kept.size)
        assertTrue(warnings.isEmpty())
    }

    @Test
    fun `the picker presents a pending share once`() {
        var state = IncomingSharePresentationState()

        var transition =
            transitionIncomingSharePresentation(
                state,
                isInShareFlow = false,
                isOnProjectPicker = false,
                pendingShareId = "share-1",
            )
        assertEquals("share-1", transition.shareIdToPresent)
        state = transition.state

        // Arriving at the picker latches the presentation.
        transition =
            transitionIncomingSharePresentation(
                state,
                isInShareFlow = true,
                isOnProjectPicker = true,
                pendingShareId = "share-1",
            )
        assertNull(transition.shareIdToPresent)
        state = transition.state

        // Leaving the flow with the share still pending dismisses it.
        transition =
            transitionIncomingSharePresentation(
                state,
                isInShareFlow = false,
                isOnProjectPicker = false,
                pendingShareId = "share-1",
            )
        assertNull(transition.shareIdToPresent)
        state = transition.state

        // It is not re-presented.
        transition =
            transitionIncomingSharePresentation(
                state,
                isInShareFlow = false,
                isOnProjectPicker = false,
                pendingShareId = "share-1",
            )
        assertNull(transition.shareIdToPresent)
        assertNull(transition.shareIdToPresent)

        // A different share still presents.
        transition =
            transitionIncomingSharePresentation(
                transition.state,
                isInShareFlow = false,
                isOnProjectPicker = false,
                pendingShareId = "share-2",
            )
        assertEquals("share-2", transition.shareIdToPresent)
    }

    @Test
    fun `consuming the presented share while in the flow resets the latch`() {
        var state = IncomingSharePresentationState(presentedShareId = "share-1")
        val transition =
            transitionIncomingSharePresentation(
                state,
                isInShareFlow = true,
                isOnProjectPicker = false,
                pendingShareId = null,
            )
        assertNull(transition.shareIdToPresent)
        assertNull(transition.state.presentedShareId)
        state = transition.state
        assertNull(state.dismissedShareId)
    }
}
