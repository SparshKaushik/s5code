package club.touchtech.s5code.kotlin.feature.home

import org.junit.Test
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue

/**
 * The order-key algebra, mirroring `threadSort.test.ts` in client-runtime: a
 * move is a single write, a keyless neighbor respreads the section, and a key
 * reserved by a hidden row is never reused.
 */
class ThreadSortTest {
    @Test
    fun `midpoint keys sort strictly between their bounds`() {
        val key = pinOrderKeyBetween("f", "t") ?: error("expected a midpoint")
        assertTrue(key > "f" && key < "t")
    }

    @Test
    fun `corrupt or out-of-order bounds refuse to generate`() {
        assertNull(pinOrderKeyBetween("t", "f"))
        // A trailing minimum digit leaves no room below it.
        assertNull(pinOrderKeyBetween("ma", null))
    }

    @Test
    fun `inserting between neighbors avoids a hidden row's key`() {
        val midpoint = pinOrderKeyBetween("f", "t")!!
        val assignments =
            planReorder(
                orderedIds = listOf("a", "moved", "b"),
                keysById = mapOf("a" to "f", "b" to "t", "moved" to "z", "snoozed" to midpoint),
                movedId = "moved",
            )
        assertEquals(1, assignments.size)
        assertEquals("moved", assignments[0].id)
        assertTrue(assignments[0].orderKey > "f" && assignments[0].orderKey < "t")
        assertTrue(assignments[0].orderKey != midpoint)
    }

    @Test
    fun `a keyless neighbor respreads the whole section in the new order`() {
        val reserved = generateSpreadPinOrderKeys(6)
        val keysById: MutableMap<String, String?> =
            mutableMapOf<String, String?>("a" to null, "b" to null, "c" to null).apply {
                reserved.forEachIndexed { index, key -> put("hidden-$index", key) }
            }
        val assignments = planReorder(listOf("c", "a", "b"), keysById, "c")
        assertEquals(listOf("c", "a", "b"), assignments.map { it.id })
        val keys = assignments.map { it.orderKey }
        assertEquals(keys.sorted(), keys)
        assertEquals(3, keys.toSet().size)
        assertTrue(keys.none { it in reserved })
    }

    @Test
    fun `move up writes one key`() {
        val assignments =
            planMove(
                orderedIds = listOf("a", "b", "c"),
                keysById = mapOf("a" to "f", "b" to "m", "c" to "t"),
                movedId = "c",
                direction = MoveDirection.Up,
            ) ?: error("expected a move plan")
        assertEquals(1, assignments.size)
        assertEquals("c", assignments[0].id)
        assertTrue(assignments[0].orderKey > "f" && assignments[0].orderKey < "m")
    }

    @Test
    fun `moves off either end refuse`() {
        val keys = mapOf("a" to "f", "b" to "m")
        assertNull(planMove(listOf("a", "b"), keys, "a", MoveDirection.Up))
        assertNull(planMove(listOf("a", "b"), keys, "b", MoveDirection.Down))
    }

    @Test
    fun `a move planning writes for a neighbor on a non-reorderable environment is refused`() {
        val ordered =
            listOf(
                ThreadOrderRow("e1:a", "e1", null),
                ThreadOrderRow("e1:b", "e1", "m"),
                ThreadOrderRow("e2:c", "e2", null),
            )
        // e2 cannot take key writes, so moving e2:c up (which respreads because
        // its neighbor is keyless) must not be offered.
        assertNull(
            planThreadMove(
                ordered = ordered,
                allThreads = ordered,
                reorderableEnvironmentIds = setOf("e1"),
                movedCompositeId = "e2:c",
                direction = MoveDirection.Up,
            )
        )
        // The same row on a capable environment plans.
        assertNotNull(
            planThreadMove(
                ordered = ordered,
                allThreads = ordered,
                reorderableEnvironmentIds = setOf("e1", "e2"),
                movedCompositeId = "e2:c",
                direction = MoveDirection.Up,
            )
        )
    }
}
