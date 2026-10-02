package club.touchtech.s5code.kotlin.feature.onboarding

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.material.icons.rounded.Key
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.cloud.CloudAccountState
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5ShapeBadge
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
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
