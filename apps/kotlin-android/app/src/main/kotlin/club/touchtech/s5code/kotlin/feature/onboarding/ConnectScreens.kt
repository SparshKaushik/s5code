package club.touchtech.s5code.kotlin.feature.onboarding

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Login
import androidx.compose.material.icons.automirrored.rounded.Logout
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Key
import androidx.compose.material.icons.rounded.Link
import androidx.compose.material.icons.rounded.PhoneAndroid
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.cloud.CloudAccountState
import club.touchtech.s5code.kotlin.cloud.CloudDeviceRow
import club.touchtech.s5code.kotlin.cloud.CloudEnvironmentRow
import club.touchtech.s5code.kotlin.cloud.CloudEnvironmentsState
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5ListRow
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5RowPosition
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5ShapeBadge
import club.touchtech.s5code.kotlin.design.component.S5StatusPill
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5MaterialShapes
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import com.clerk.ui.auth.AuthView

/**
 * S5 Connect sign-in.
 *
 * Signing in is Clerk's own [AuthView] rather than a hand-rolled form: it carries
 * whatever strategies the S5 Clerk instance has enabled (email code, password,
 * Google, passkeys), and reproducing that here would drift the moment the
 * dashboard changes. The screen around it owns the three states the account can
 * be in — unconfigured build, signed out, signed in — because those decide
 * navigation, not the view.
 */
@Composable
fun ConnectSignInScreen(store: AppStore, onBack: () -> Unit, onContinue: () -> Unit) {
    val account by store.cloud.state.collectAsStateWithLifecycle()
    var authOpen by remember { mutableStateOf(false) }

    // Closing on success rather than in composition: writing state while composing
    // is what makes a recomposition loop.
    LaunchedEffect(account) {
        if (account is CloudAccountState.SignedIn) authOpen = false
    }

    // Auth takes the whole screen while it is open, including its own back
    // affordance: nesting Clerk's flow inside our chrome would give two headers
    // and two back buttons for one task.
    if (authOpen && account is CloudAccountState.SignedOut) {
        AuthView(isDismissible = true, onDismiss = { authOpen = false })
        return
    }

    when (val current = account) {
        CloudAccountState.Unconfigured ->
            ConnectDisabledScreen(onBack = onBack)
        CloudAccountState.Loading ->
            S5Screen(title = "S5 Connect", onBack = onBack) { padding ->
                S5LoadingState("Checking your account…", Modifier.padding(padding))
            }
        is CloudAccountState.SignedOut ->
            ConnectAccountScreen(
                title = "Sign in to S5",
                detail = "Sign-in opens Clerk on this device. We never see your provider credentials.",
                error = current.error,
                onBack = onBack,
                actions = {
                    S5Button(
                        text = "Sign in",
                        onClick = { authOpen = true },
                        emphasis = S5ActionEmphasis.Primary,
                        icon = Icons.AutoMirrored.Rounded.Login,
                    )
                },
            )
        is CloudAccountState.SignedIn ->
            ConnectAccountScreen(
                title = current.label,
                detail =
                    "Your account is what will link this device to machines you enroll with " +
                        "the relay.",
                error = null,
                onBack = onBack,
                signedIn = true,
                actions = {
                    S5Button(
                        text = "Continue",
                        onClick = onContinue,
                        emphasis = S5ActionEmphasis.Primary,
                    )
                    S5Button(
                        text = "Sign out",
                        onClick = store.cloud::signOut,
                        emphasis = S5ActionEmphasis.Primary,
                        style = S5ButtonStyle.Outlined,
                        icon = Icons.AutoMirrored.Rounded.Logout,
                    )
                },
            )
    }
}

/**
 * What a build with no Clerk or relay configuration shows. A fork that never set
 * the public config still needs an honest screen here, and "sign in" would be a
 * button that cannot work.
 */
@Composable
private fun ConnectDisabledScreen(onBack: () -> Unit) {
    S5Screen(
        title = "S5 Connect",
        subtitle = "Not configured in this build",
        prominence = S5TopBarProminence.Hero,
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).padding(horizontal = S5Theme.spacing.gutter),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            S5Card(tone = S5CardTone.Hero, modifier = Modifier.fillMaxWidth()) {
                Column(
                    Modifier.fillMaxWidth().padding(S5Theme.spacing.xLarge),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                ) {
                    S5ShapeBadge(
                        icon = Icons.Rounded.CloudOff,
                        contentDescription = null,
                        shape = S5MaterialShapes.hero(),
                        containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                        contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                        size = 96.dp,
                        iconSize = 44.dp,
                    )
                    Text(
                        "Connect is off in this build",
                        style = MaterialTheme.typography.headlineSmallEmphasized,
                        textAlign = TextAlign.Center,
                    )
                    Text(
                        "This APK was built without the Clerk and relay configuration, so " +
                            "remote access through S5 Connect is unavailable. Pair directly over " +
                            "your network instead.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                }
            }
            S5Notice(
                icon = Icons.Rounded.Key,
                text = "Direct pairing needs no account: the token stays on this device.",
            )
        }
    }
}

/** Shared shell for the signed-out and signed-in states. */
@Composable
private fun ConnectAccountScreen(
    title: String,
    detail: String,
    error: String?,
    onBack: () -> Unit,
    /** Only changes the subtitle; the caller owns which actions are offered. */
    signedIn: Boolean = false,
    actions: @Composable () -> Unit,
) {
    S5Screen(
        title = "S5 Connect",
        subtitle = if (signedIn) "Signed in" else "Reach your machines from anywhere",
        prominence = S5TopBarProminence.Hero,
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                S5Card(tone = S5CardTone.Hero, modifier = Modifier.fillMaxWidth()) {
                    Column(
                        Modifier.fillMaxWidth().padding(S5Theme.spacing.xLarge),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                    ) {
                        S5ShapeBadge(
                            icon = Icons.Rounded.Cloud,
                            contentDescription = null,
                            shape = S5MaterialShapes.hero(),
                            containerColor = MaterialTheme.colorScheme.primaryContainer,
                            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                            size = 96.dp,
                            iconSize = 44.dp,
                        )
                        Text(
                            title,
                            style = MaterialTheme.typography.headlineSmallEmphasized,
                            textAlign = TextAlign.Center,
                        )
                        Text(
                            detail,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            textAlign = TextAlign.Center,
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                            actions()
                        }
                    }
                }
            }

            if (error != null) {
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Notice(
                        icon = Icons.Rounded.CloudOff,
                        text = error,
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                        contentColor = MaterialTheme.colorScheme.onErrorContainer,
                    )
                }
            }

            Box(
                Modifier.padding(
                    start = S5Theme.spacing.gutter,
                    end = S5Theme.spacing.gutter,
                    bottom = S5Theme.spacing.section,
                )
            ) {
                S5Notice(
                    icon = Icons.Rounded.Key,
                    text =
                        "Relay credentials are bound to a device key (DPoP) so a stolen token " +
                            "is useless elsewhere.",
                )
            }
        }
    }
}

/**
 * Managed environments.
 *
 * This is the second half of S5 Connect: the account is signed in, and this screen
 * lists the machines the relay has a live link for so one can be added to this
 * device. Adding is not "pairing" in the direct sense — no credential is stored.
 * The row records the link, and every connection mints a fresh credential bound
 * to this device's key.
 *
 * Health arrives after the list on purpose: five status probes run independently,
 * so a sleeping desktop shows as offline without delaying the laptop next to it.
 */
@Composable
fun ConnectSetupScreen(store: AppStore, onBack: () -> Unit, onDone: () -> Unit) {
    val account by store.cloud.state.collectAsStateWithLifecycle()
    val environments = store.cloudEnvironments

    if (environments == null) {
        ConnectDisabledScreen(onBack = onBack)
        return
    }
    if (account !is CloudAccountState.SignedIn) {
        ConnectSignedOutSetupScreen(onBack = onBack)
        return
    }

    val state by environments.state.collectAsStateWithLifecycle()
    val linking by environments.linking.collectAsStateWithLifecycle()
    val linkError by environments.linkError.collectAsStateWithLifecycle()

    // One load per visit to this screen. Refreshing on every recomposition would
    // put two relay round trips behind every health dot repaint.
    LaunchedEffect(Unit) { environments.refresh() }

    S5Screen(
        title = "Your machines",
        subtitle = (account as? CloudAccountState.SignedIn)?.label ?: "",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
        actions = {
            S5IconButton(
                icon = Icons.Rounded.Refresh,
                label = "Refresh",
                onClick = environments::refresh,
            )
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            if (linkError != null) {
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5ErrorState(
                        title = "Couldn't add that machine",
                        detail = linkError.orEmpty(),
                        onRetry = environments::clearLinkError,
                        retryLabel = "Dismiss",
                    )
                }
            }
            when (val current = state) {
                CloudEnvironmentsState.Idle,
                CloudEnvironmentsState.Loading -> S5LoadingState("Asking the relay…")
                is CloudEnvironmentsState.Failed ->
                    Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                        S5ErrorState(
                            title = "Couldn't reach the relay",
                            detail = current.message,
                            onRetry = environments::refresh,
                        )
                    }
                is CloudEnvironmentsState.Loaded ->
                    LazyColumn(
                        Modifier.fillMaxSize(),
                        contentPadding =
                            PaddingValues(
                                start = S5Theme.spacing.gutter,
                                end = S5Theme.spacing.gutter,
                                bottom = S5Theme.spacing.section,
                            ),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    ) {
                        if (current.rows.isEmpty()) {
                            // Inline rather than a full-screen empty state: an
                            // account with no machines can still have devices
                            // below, and covering the screen would hide them.
                            item(key = "connect-machines-empty") {
                                S5Card(tone = S5CardTone.Receded, modifier = Modifier.fillMaxWidth()) {
                                    Column(
                                        Modifier.fillMaxWidth().padding(S5Theme.spacing.large),
                                        verticalArrangement =
                                            Arrangement.spacedBy(S5Theme.spacing.small),
                                    ) {
                                        Text(
                                            "No machines enrolled",
                                            style = MaterialTheme.typography.titleMedium,
                                        )
                                        Text(
                                            "Run S5 Code on a machine and enable S5 Connect " +
                                                "there. It will appear here for this account.",
                                            style = MaterialTheme.typography.bodyMedium,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                        S5Button(
                                            text = "Check again",
                                            onClick = environments::refresh,
                                            emphasis = S5ActionEmphasis.Prominent,
                                            style = S5ButtonStyle.Tonal,
                                        )
                                    }
                                }
                            }
                        } else {
                            items(current.rows, key = { it.environmentId }) { row ->
                                CloudEnvironmentCard(
                                    row = row,
                                    busy = linking == row.environmentId,
                                    onAdd = { environments.link(row.environmentId, onDone) },
                                    onOpen = onDone,
                                )
                            }
                        }

                        // Devices come after machines: they are context for the
                        // account, not something to act on here.
                        item(key = "connect-devices-header") { S5SectionHeader("Your devices") }
                        when {
                            current.devicesError != null ->
                                item(key = "connect-devices-error") {
                                    S5Notice(
                                        icon = Icons.Rounded.Devices,
                                        text = current.devicesError,
                                    )
                                }
                            current.devices.isEmpty() ->
                                item(key = "connect-devices-empty") {
                                    S5Notice(
                                        icon = Icons.Rounded.Devices,
                                        text =
                                            "No devices are registered for notifications yet. " +
                                                "Signing in on a phone registers it here.",
                                    )
                                }
                            else ->
                                itemsIndexed(
                                    current.devices,
                                    key = { _, device -> device.deviceId },
                                ) { index, device ->
                                    CloudDeviceCard(
                                        device = device,
                                        position = rowPosition(index, current.devices.size),
                                    )
                                }
                        }
                    }
            }
        }
    }
}

/**
 * One client registered to the account: what it is, and what it will be told
 * about.
 *
 * Read-only on purpose. The relay supports unregistering a device, but doing it
 * from here would let a phone silence a laptop's notifications with one tap and
 * no way to undo it — that belongs where the device's own settings are.
 */
@Composable
private fun CloudDeviceCard(device: CloudDeviceRow, position: S5RowPosition) {
    S5ListRow(
        icon = Icons.Rounded.PhoneAndroid,
        label = device.label,
        supporting = "${device.platform} · ${device.notifications}",
        meta = device.updated,
        position = position,
    )
}

/** One managed machine: identity, health, and the one action it supports. */
@Composable
private fun CloudEnvironmentCard(
    row: CloudEnvironmentRow,
    busy: Boolean,
    onAdd: () -> Unit,
    onOpen: () -> Unit,
) {
    S5Card(
        tone = if (row.online == false) S5CardTone.Receded else S5CardTone.Standard,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            Modifier.fillMaxWidth().padding(S5Theme.spacing.large),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                S5ShapeBadge(
                    icon = Icons.Rounded.Cloud,
                    contentDescription = null,
                    shape = S5MaterialShapes.avatar(),
                    containerColor = MaterialTheme.colorScheme.secondaryContainer,
                    contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                    size = 44.dp,
                    iconSize = 22.dp,
                )
                Column(Modifier.weight(1f)) {
                    Text(row.label, style = MaterialTheme.typography.titleMedium)
                    Text(
                        row.host,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                when (row.online) {
                    // The pending state is a label, not a spinner: a row that
                    // animates while it waits for a health probe is exactly the
                    // idle repaint the perf rules call out.
                    null ->
                        S5StatusPill(
                            label = "Checking",
                            containerColor = MaterialTheme.colorScheme.surfaceContainerHighest,
                            contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    true ->
                        S5StatusPill(
                            label = "Online",
                            containerColor = MaterialTheme.colorScheme.primaryContainer,
                            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                        )
                    false ->
                        S5StatusPill(
                            label = "Offline",
                            containerColor = MaterialTheme.colorScheme.surfaceContainerHighest,
                            contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                }
            }
            row.statusError?.let { message ->
                Text(
                    message,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (row.linked) {
                S5Button(
                    text = "Open",
                    onClick = onOpen,
                    emphasis = S5ActionEmphasis.Prominent,
                    style = S5ButtonStyle.Tonal,
                )
            } else {
                S5Button(
                    text = if (busy) "Adding…" else "Add to this device",
                    onClick = onAdd,
                    emphasis = S5ActionEmphasis.Prominent,
                    icon = Icons.Rounded.Link,
                    enabled = !busy,
                )
            }
        }
    }
}

/** Reached by a deep link into setup while signed out. */
@Composable
private fun ConnectSignedOutSetupScreen(onBack: () -> Unit) {
    S5Screen(title = "Your machines", subtitle = "Signed out", onBack = onBack) { padding ->
        S5EmptyState(
            icon = Icons.Rounded.CloudOff,
            title = "Sign in first",
            detail = "S5 Connect lists the machines linked to your account.",
            modifier = Modifier.padding(padding),
        )
    }
}
