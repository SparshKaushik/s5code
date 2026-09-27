package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.lazy.LazyListState

/**
 * A remembered scroll position, for restoring across a content mutation.
 *
 * [key] is the list item to anchor on; [scrollOffset] is the value
 * `LazyListState.scrollToItem` would take to reproduce where that item sat —
 * the distance its start edge is scrolled past the viewport's start edge
 * (`-item.offset` in the scroll-space coordinates `LazyListItemInfo` reports,
 * before `reverseLayout`'s flip). Zero is a fully visible item at the start
 * edge; negative puts it inside the viewport.
 */
data class ScrollAnchor(
    val key: Any,
    val scrollOffset: Int,
)

/**
 * Where the viewport sits right now, anchored on the first visible item.
 * Null when nothing has been laid out.
 */
fun LazyListState.scrollAnchor(): ScrollAnchor? {
    val first = layoutInfo.visibleItemsInfo.firstOrNull() ?: return null
    return ScrollAnchor(key = first.key, scrollOffset = -first.offset)
}

/**
 * Snap [anchor]'s item back to where it was, once [rows] hold it again.
 * When the item itself was removed (a collapsed section took its content
 * with it), [fallbackKey] — usually the toggled row's own key — scrolls
 * to the start edge instead. No-op when neither survived.
 */
suspend fun LazyListState.scrollToAnchor(anchor: ScrollAnchor, keys: List<Any?>, fallbackKey: Any? = null) {
    val index = keys.indexOf(anchor.key)
    if (index >= 0) {
        scrollToItem(index, anchor.scrollOffset)
    } else if (fallbackKey != null) {
        val fallback = keys.indexOf(fallbackKey)
        if (fallback >= 0) scrollToItem(fallback, 0)
    }
}
