package club.touchtech.s5code.kotlin.feature.home

/**
 * Fractional order keys for user-arranged threads, ported from
 * `packages/client-runtime/src/state/threadSort.ts` and the move planner in
 * `apps/mobile/src/features/threads/threadOrder.ts`.
 *
 * Pinned and active threads carry an optional base-26 order key written by the
 * client that arranged them. A move writes one key to one thread on that
 * thread's own server — neighbors, possibly living on other servers, are never
 * touched — so every client connected to the same servers converges on the same
 * order.
 */

private const val PIN_ORDER_DIGITS = "abcdefghijklmnopqrstuvwxyz"

private fun isValidPinOrderKey(key: String): Boolean {
    if (key.isEmpty()) return false
    for (char in key) {
        if (char !in PIN_ORDER_DIGITS) return false
    }
    // A trailing minimum digit would leave no room to sort a key immediately
    // before this one; generators never produce it, so treat it as corrupt.
    return key.last() != PIN_ORDER_DIGITS[0]
}

/** Midpoint of two digit strings interpreted as fractions in (0, 1). "" stands for the open bound. Requires a < b. */
private fun pinOrderMidpoint(a: String, b: String): String {
    if (b.isNotEmpty()) {
        check(a < b) { "pinOrderMidpoint: bounds out of order" }
        // Recurse past the longest common prefix ("a" pads the shorter side).
        var n = 0
        while ((a.getOrNull(n) ?: PIN_ORDER_DIGITS[0]) == b[n]) n += 1
        if (n > 0) return b.substring(0, n) + pinOrderMidpoint(a.substring(n), b.substring(n))
    }
    val digitA = if (a.isEmpty()) 0 else PIN_ORDER_DIGITS.indexOf(a[0])
    val digitB = if (b.isEmpty()) PIN_ORDER_DIGITS.length else PIN_ORDER_DIGITS.indexOf(b[0])
    if (digitB - digitA > 1) {
        return PIN_ORDER_DIGITS[(digitA + digitB + 1) / 2].toString()
    }
    // Consecutive leading digits: either b has spare digits to shorten into, or
    // we extend a (never producing a trailing minimum digit).
    if (b.length > 1) return b[0].toString()
    return PIN_ORDER_DIGITS[digitA].toString() + pinOrderMidpoint(a.substring(1), "")
}

/**
 * Key that sorts strictly between two neighbors; null bounds mean "top of the
 * pinned block" / "bottom of the keyed run". Returns null instead of throwing
 * when existing keys are corrupt or out of order — callers fall back to
 * rewriting the section.
 */
fun pinOrderKeyBetween(before: String?, after: String?): String? {
    val a = before.orEmpty()
    val b = after.orEmpty()
    if (a.isNotEmpty() && !isValidPinOrderKey(a)) return null
    if (b.isNotEmpty() && !isValidPinOrderKey(b)) return null
    if (b.isNotEmpty() && a >= b) return null
    return pinOrderMidpoint(a, b)
}

/**
 * Evenly spaced keys for materializing an order. Wider keys keep a large
 * active list from exhausting the space between two-digit keys.
 */
fun generateSpreadPinOrderKeys(count: Int): List<String> {
    var width = 2
    var space = PIN_ORDER_DIGITS.length * PIN_ORDER_DIGITS.length
    while (space <= (count + 1) * 2) {
        width += 1
        space *= PIN_ORDER_DIGITS.length
    }
    val step = space.toDouble() / (count + 1)
    val keys = mutableListOf<String>()
    for (i in 0 until count) {
        var value = Math.round(step * (i + 1)).toInt()
        // Skip values whose low digit is the minimum (a trailing "a" key).
        if (value % PIN_ORDER_DIGITS.length == 0) value += 1
        var key = ""
        repeat(width) {
            key = PIN_ORDER_DIGITS[value % PIN_ORDER_DIGITS.length] + key
            value /= PIN_ORDER_DIGITS.length
        }
        keys.add(key)
    }
    return keys
}

data class ThreadOrderAssignment(val id: String, val orderKey: String)

/**
 * Assignments needed to realize a new order. When the moved thread sits
 * between two keyed (or absent) neighbors, this is a single write. When a
 * neighbor is keyless (threads that predate reordering), the whole section
 * gets fresh spread keys — a one-time materialization; every move after that
 * is single-write. Ported from `planPinnedReorder`; the same planner serves
 * the pinned and active sections with different key fields.
 */
fun planReorder(
    /** Thread ids in the desired visual order (after the move). */
    orderedIds: List<String>,
    /** Retained keys from hidden rows too; only orderedIds receive writes. */
    keysById: Map<String, String?>,
    movedId: String,
): List<ThreadOrderAssignment> {
    val visibleIds = orderedIds.toSet()
    val reservedKeys =
        keysById
            .filter { (id, key) -> id !in visibleIds && key != null }
            .values
            .filterNotNull()
            .toSet()
    val movedIndex = orderedIds.indexOf(movedId)
    if (movedIndex == -1) return emptyList()
    val beforeId = orderedIds.getOrNull(movedIndex - 1)
    val afterId = orderedIds.getOrNull(movedIndex + 1)
    val beforeKey = beforeId?.let { keysById[it] }
    val afterKey = afterId?.let { keysById[it] }
    val beforeUsable = beforeId == null || beforeKey != null
    val afterUsable = afterId == null || afterKey != null
    if (beforeUsable && afterUsable) {
        var key = pinOrderKeyBetween(beforeKey, afterKey)
        while (key != null && key in reservedKeys) key = pinOrderKeyBetween(key, afterKey)
        if (key != null) return listOf(ThreadOrderAssignment(movedId, key))
    }
    // Keyless neighbor (or corrupt keys): rewrite the section in the new order.
    val keys =
        generateSpreadPinOrderKeys(orderedIds.size + reservedKeys.size)
            .filter { it !in reservedKeys }
            .take(orderedIds.size)
    return orderedIds.mapIndexedNotNull { index, id ->
        val key = keys[index]
        if (keysById[id] == key) null else ThreadOrderAssignment(id, key)
    }
}

/**
 * `planPinnedMove`: swap the moved thread with its displayed neighbor. Null
 * when the move falls off either end of the list.
 */
fun planMove(
    orderedIds: List<String>,
    keysById: Map<String, String?>,
    movedId: String,
    direction: MoveDirection,
): List<ThreadOrderAssignment>? {
    val from = orderedIds.indexOf(movedId)
    if (from == -1) return null
    val to = if (direction == MoveDirection.Up) from - 1 else from + 1
    if (to < 0 || to >= orderedIds.size) return null
    val newOrder = orderedIds.toMutableList()
    newOrder.removeAt(from)
    newOrder.add(to, movedId)
    return planReorder(newOrder, keysById, movedId)
}

enum class MoveDirection {
    Up,
    Down,
}

/**
 * The rows a section sorts by and the key each carries, generalized over
 * pinned (`pinOrderKey`) and active (`activeOrderKey`) — `OrderRow` in the RN
 * planner. Ids are composite (`environmentId:threadId`) because thread ids are
 * only unique within an environment and the merged list mixes them.
 */
data class ThreadOrderRow(
    val compositeId: String,
    val environmentId: String,
    val orderKey: String?,
)

/**
 * `createThreadMovePlanner`: every visible row stays an anchor, but plans are
 * only offered when every key write lands on a reorder-capable environment —
 * menu availability and execution share this planner.
 */
fun planThreadMove(
    ordered: List<ThreadOrderRow>,
    allThreads: List<ThreadOrderRow>,
    reorderableEnvironmentIds: Set<String>,
    movedCompositeId: String,
    direction: MoveDirection,
): List<ThreadOrderAssignment>? {
    val writableIds =
        allThreads
            .filter { it.environmentId in reorderableEnvironmentIds }
            .mapTo(mutableSetOf()) { it.compositeId }
    if (movedCompositeId !in writableIds) return null
    val assignments =
        planMove(
            orderedIds = ordered.map { it.compositeId },
            keysById = allThreads.associate { it.compositeId to it.orderKey },
            movedId = movedCompositeId,
            direction = direction,
        ) ?: return null
    if (assignments.isEmpty()) return null
    if (assignments.any { it.id !in writableIds }) return null
    return assignments
}
