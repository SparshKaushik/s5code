package club.touchtech.s5code.kotlin.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Cache inventory presentation.
 *
 * This exists because the Client storage screen used to report four hardcoded
 * megabyte figures. The rules worth pinning are the ones that keep the replacement
 * honest: zero says "Empty" rather than inviting a tap, and the label never rounds a
 * real number away.
 */
class ClientCacheTest {

    @Test
    fun `zero bytes reads as empty rather than a size`() {
        // A row reading "0 B" invites a clear that does nothing.
        assertEquals("Empty", cacheSizeLabel(0))
        assertEquals("Empty", cacheSizeLabel(-1))
    }

    @Test
    fun `small sizes stay in bytes`() {
        assertEquals("1 B", cacheSizeLabel(1))
        assertEquals("1023 B", cacheSizeLabel(1023))
    }

    @Test
    fun `binary units, one decimal, whole numbers without it`() {
        assertEquals("1 KB", cacheSizeLabel(1024))
        assertEquals("1.5 KB", cacheSizeLabel(1536))
        assertEquals("1 MB", cacheSizeLabel(1024L * 1024))
        assertEquals("18.4 MB", cacheSizeLabel((18.4 * 1024 * 1024).toLong()))
        assertEquals("1 GB", cacheSizeLabel(1024L * 1024 * 1024))
    }

    @Test
    fun `three digits drop the decimal, which is noise at that scale`() {
        assertEquals("512 MB", cacheSizeLabel(512L * 1024 * 1024))
    }

    @Test
    fun `the largest unit is GB rather than overflowing the list`() {
        assertEquals("2 GB", cacheSizeLabel(2L * 1024 * 1024 * 1024))
        assertEquals("1024 GB", cacheSizeLabel(1024L * 1024 * 1024 * 1024))
    }

    @Test
    fun `an inventory totals its entries and knows when it is empty`() {
        val empty =
            ClientCacheInventory(
                ClientCacheKind.entries.map { ClientCacheEntry(it, bytes = 0) }
            )
        assertTrue(empty.isEmpty)
        assertEquals(0L, empty.totalBytes)

        val filled =
            ClientCacheInventory(
                listOf(
                    ClientCacheEntry(ClientCacheKind.Attachments, bytes = 1024),
                    ClientCacheEntry(ClientCacheKind.Images, bytes = 2048),
                    ClientCacheEntry(ClientCacheKind.Workspace, bytes = 4096),
                )
            )
        assertFalse(filled.isEmpty)
        assertEquals(7168L, filled.totalBytes)
    }

    @Test
    fun `every category names itself and explains what it holds`() {
        // The screen renders these directly, so a new kind cannot ship unlabelled.
        ClientCacheKind.entries.forEach { kind ->
            val entry = ClientCacheEntry(kind, bytes = 0)
            assertTrue(entry.label.isNotBlank())
            assertTrue(entry.detail.isNotBlank())
        }
    }
}
