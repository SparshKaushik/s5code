package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SheetValue
import androidx.compose.material3.Text
import androidx.compose.material3.rememberBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import club.touchtech.s5code.kotlin.design.theme.S5Theme

/**
 * The one modal sheet feature code uses.
 *
 * Sheets are for picking something and getting back to work: they keep the
 * caller's screen (and its scroll position, and the transcript you were reading)
 * behind the scrim instead of pushing a destination you then have to back out
 * of. A picker that lives on the back stack costs a full transition each way and
 * loses the context you opened it from.
 *
 * Reachability is why this does not skip the partially-expanded state: a tall
 * sheet that opens fully expanded puts its first row under your thumb and its
 * last row out of reach.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun S5BottomSheet(
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    title: String? = null,
    subtitle: String? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    val state = rememberBottomSheetState(initialValue = SheetValue.Hidden)
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = state, modifier = modifier) {
        Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
            if (title != null) {
                Column(
                    Modifier.padding(
                        start = S5Theme.spacing.gutter,
                        end = S5Theme.spacing.gutter,
                        bottom = S5Theme.spacing.small,
                    )
                ) {
                    Text(
                        title,
                        style = MaterialTheme.typography.titleMediumEmphasized,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (subtitle != null) {
                        Text(
                            subtitle,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
            content()
        }
    }
}
