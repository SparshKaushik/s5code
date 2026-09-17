package club.touchtech.s5code.kotlin.feature.onboarding

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Bolt
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.ContentPaste
import androidx.compose.material.icons.rounded.Lan
import androidx.compose.material.icons.rounded.Link
import androidx.compose.material.icons.rounded.Lock
import androidx.compose.material.icons.rounded.QrCodeScanner
import androidx.compose.material.icons.rounded.Router
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.connection.PairingUrlResult
import club.touchtech.s5code.kotlin.connection.extractPairingUrlFromQrPayload
import club.touchtech.s5code.kotlin.connection.pairingTargetFor
import club.touchtech.s5code.kotlin.connection.parsePairingUrl
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5CodeBlock
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SectionHeader
import club.touchtech.s5code.kotlin.design.component.S5ShapeBadge
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.theme.S5MaterialShapes
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.platform.QrScannerPreview
import kotlinx.coroutines.launch

/**
 * First-run explainer. Two ways in — direct pairing on your own network, or S5
 * Connect for anywhere access — with the hero action on the path most users
 * take.
 */
@Composable
fun OnboardingScreen(
    onPairUrl: () -> Unit,
    onPairQr: () -> Unit,
    onConnect: () -> Unit,
) {
    ConnectionChoices(
        onPairUrl = onPairUrl,
        onPairQr = onPairQr,
        onConnect = onConnect,
        header = {
            Column(
                Modifier.fillMaxWidth().padding(top = 72.dp, bottom = S5Theme.spacing.large),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                S5ShapeBadge(
                    icon = Icons.Rounded.Terminal,
                    contentDescription = null,
                    shape = S5MaterialShapes.hero(),
                    containerColor = MaterialTheme.colorScheme.primaryContainer,
                    contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                    size = 112.dp,
                    iconSize = 48.dp,
                )
                Text(
                    "Drive your agents from anywhere",
                    style = MaterialTheme.typography.headlineMediumEmphasized,
                    textAlign = TextAlign.Center,
                    modifier =
                        Modifier.padding(
                            top = S5Theme.spacing.xLarge,
                            start = S5Theme.spacing.xxLarge,
                            end = S5Theme.spacing.xxLarge,
                        ),
                )
                Text(
                    "S5 Code runs on your machine with your own subscriptions. This app connects to it.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                    modifier =
                        Modifier.padding(
                            top = S5Theme.spacing.small,
                            start = S5Theme.spacing.xxLarge,
                            end = S5Theme.spacing.xxLarge,
                        ),
                )
            }
        },
    )
}

/**
 * Adding a second (or tenth) environment from Connections.
 *
 * The same two choices as first run, because they are the same two choices; the
 * difference is that this one is a pushed destination with a bar and a way back,
 * and it skips the first-run pitch. Sending Connections straight to the URL form
 * instead would silently drop S5 Connect as an option once a device is paired.
 */
@Composable
fun AddEnvironmentScreen(
    onBack: () -> Unit,
    onPairUrl: () -> Unit,
    onPairQr: () -> Unit,
    onConnect: () -> Unit,
) {
    S5Screen(
        title = "Add environment",
        subtitle = "Connect another machine",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
    ) { padding ->
        ConnectionChoices(
            onPairUrl = onPairUrl,
            onPairQr = onPairQr,
            onConnect = onConnect,
            modifier = Modifier.padding(padding),
        )
    }
}

/** The two ways in, shared by first run and Connections' add flow. */
@Composable
private fun ConnectionChoices(
    onPairUrl: () -> Unit,
    onPairQr: () -> Unit,
    onConnect: () -> Unit,
    modifier: Modifier = Modifier,
    header: (@Composable () -> Unit)? = null,
) {
    Column(
        modifier.fillMaxSize().verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
    ) {
        header?.invoke()
        if (header == null) Box(Modifier.padding(top = S5Theme.spacing.small))

        Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
            ConnectionChoiceCard(
                icon = Icons.Rounded.Lan,
                title = "Direct connection",
                detail =
                    "Fastest and fully local. Your phone talks straight to the machine over " +
                        "your network, a tailnet, or a tunnel.",
                bullets =
                    listOf(
                        "Run npx s5 on your machine",
                        "Paste the pairing URL it prints, or scan the QR code",
                        "Nothing leaves your network",
                    ),
                tone = S5CardTone.Hero,
                primaryLabel = "Paste a pairing URL",
                onPrimary = onPairUrl,
                secondaryLabel = "Scan QR code",
                secondaryIcon = Icons.Rounded.QrCodeScanner,
                onSecondary = onPairQr,
            )
        }

        Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
            ConnectionChoiceCard(
                icon = Icons.Rounded.Cloud,
                title = "S5 Connect",
                detail =
                    "Reach your machine from anywhere without opening ports. Traffic passes " +
                        "through our relay; your code and credentials stay on your machine.",
                bullets =
                    listOf(
                        "Sign in once on this device",
                        "Link the machines you want to reach",
                        "Works on cellular, no VPN needed",
                    ),
                tone = S5CardTone.Standard,
                primaryLabel = "Set up S5 Connect",
                onPrimary = onConnect,
            )
        }

        Box(
            Modifier.padding(
                start = S5Theme.spacing.gutter,
                end = S5Theme.spacing.gutter,
                bottom = S5Theme.spacing.section,
            )
        ) {
            S5Notice(
                icon = Icons.Rounded.Lock,
                text = "Access tokens are stored in the Android Keystore and never leave this device.",
            )
        }
    }
}

@Composable
private fun ConnectionChoiceCard(
    icon: ImageVector,
    title: String,
    detail: String,
    bullets: List<String>,
    tone: S5CardTone,
    primaryLabel: String,
    onPrimary: () -> Unit,
    secondaryLabel: String? = null,
    secondaryIcon: ImageVector? = null,
    onSecondary: (() -> Unit)? = null,
) {
    S5Card(tone = tone, modifier = Modifier.fillMaxWidth()) {
        Column(
            Modifier.padding(S5Theme.spacing.xLarge),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                S5ShapeBadge(
                    icon = icon,
                    contentDescription = null,
                    shape = S5MaterialShapes.avatar(),
                    containerColor = MaterialTheme.colorScheme.secondaryContainer,
                    contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                    size = 44.dp,
                    iconSize = 22.dp,
                )
                Text(title, style = MaterialTheme.typography.titleLargeEmphasized)
            }
            Text(
                detail,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                bullets.forEach { bullet ->
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(
                            Icons.Rounded.CheckCircle,
                            contentDescription = null,
                            modifier = Modifier.size(16.dp),
                            tint = MaterialTheme.colorScheme.primary,
                        )
                        Text(bullet, style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                S5Button(
                    text = primaryLabel,
                    onClick = onPrimary,
                    emphasis = S5ActionEmphasis.Primary,
                    icon = Icons.Rounded.Link,
                )
                if (secondaryLabel != null && onSecondary != null) {
                    S5Button(
                        text = secondaryLabel,
                        onClick = onSecondary,
                        emphasis = S5ActionEmphasis.Prominent,
                        style = S5ButtonStyle.Outlined,
                        icon = secondaryIcon,
                    )
                }
            }
        }
    }
}

/**
 * Add-by-address form, matching RN's ConnectionsNewRouteScreen: a host field
 * and a pairing-code field, with QR as the alternate entry. Pasting the whole
 * `pairingUrl` line into the host field also works — the code field stays
 * empty and the URL's own token is used.
 */
@Composable
fun PairUrlScreen(
    store: AppStore,
    onBack: () -> Unit,
    onPaired: () -> Unit,
    onScanQr: () -> Unit,
    initialHost: String = "",
    initialCode: String = "",
) {
    var host by remember { mutableStateOf(initialHost) }
    var code by remember { mutableStateOf(initialCode) }
    var attempted by remember { mutableStateOf(false) }
    var pairing by remember { mutableStateOf(false) }
    // Set only by a rejected exchange. Parse errors come from `result`, and
    // keeping the two apart is what lets editing a field clear one without
    // hiding the other.
    var failure by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    // A full pairing URL pasted into the host field wins over the split form —
    // the credential inside it is the one-time token either way.
    val result =
        remember(host, code) {
            val hostAsUrl =
                if (code.isBlank() && host.contains("://")) {
                    parsePairingUrl(host)
                } else {
                    null
                }
            hostAsUrl ?: pairingTargetFor(host, code)
        }
    val error = (result as? PairingUrlResult.Invalid)?.reason
    // Both fields blank is the resting state, not an error to wave around.
    val formEmpty = host.isBlank() && code.isBlank()

    S5Screen(
        title = "Add environment",
        subtitle = "Pair with a machine running S5 Code",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
    ) { padding ->
        Column(
            Modifier.fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = S5Theme.spacing.gutter),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            S5Card(tone = S5CardTone.Receded, modifier = Modifier.fillMaxWidth()) {
                Column(Modifier.padding(S5Theme.spacing.large)) {
                    Text(
                        "On the machine running S5 Code:",
                        style = MaterialTheme.typography.labelLargeEmphasized,
                    )
                    S5CodeBlock(
                        lines = listOf("npx s5"),
                        language = "bash",
                        modifier = Modifier.padding(top = S5Theme.spacing.small),
                    )
                    Text(
                        "The output prints the address and a one-time pairing code.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = S5Theme.spacing.small),
                    )
                }
            }

            S5TextField(
                value = host,
                onValueChange = {
                    host = it
                    attempted = false
                    failure = null
                },
                label = "Host",
                placeholder = "192.168.1.10:4488 or https://…",
                leadingIcon = Icons.Rounded.Lan,
                isError = attempted && error != null,
                supporting =
                    when {
                        attempted && error != null -> error.message
                        host.contains("://") && code.isBlank() ->
                            "Pasting the whole pairing URL here works too."
                        else -> "An IP defaults to http; a hostname defaults to https."
                    },
                maxLines = 3,
            )
            S5TextField(
                value = code,
                onValueChange = {
                    code = it
                    attempted = false
                    failure = null
                },
                label = "Pairing code",
                placeholder = "One-time token",
                leadingIcon = Icons.Rounded.Lock,
                isError = attempted && error != null,
                supporting = "Printed once; consumed the moment it pairs.",
            )

            failure?.let { message ->
                S5ErrorState(
                    title = "Pairing failed",
                    detail = message,
                    onRetry = { failure = null },
                    retryLabel = "Edit fields",
                )
            }

            Row(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                S5Button(
                    text = if (pairing) "Adding…" else "Add environment",
                    onClick = {
                        attempted = true
                        failure = null
                        val target = (result as? PairingUrlResult.Valid)?.target
                        if (target != null && !pairing) {
                            pairing = true
                            scope.launch {
                                // The credential is one-time, so the exchange writes the
                                // token before returning; there is nothing to retry with
                                // this code if it fails.
                                val outcome = runCatching { store.pairing.pair(target) }
                                pairing = false
                                outcome.fold(
                                    onSuccess = { onPaired() },
                                    onFailure = { cause ->
                                        failure = cause.message ?: "That machine refused the pairing."
                                    },
                                )
                            }
                        }
                    },
                    emphasis = S5ActionEmphasis.Primary,
                    icon = Icons.Rounded.Bolt,
                    enabled = !pairing && !formEmpty,
                )
                S5Button(
                    text = "Scan QR code",
                    onClick = onScanQr,
                    emphasis = S5ActionEmphasis.Primary,
                    style = S5ButtonStyle.Outlined,
                    icon = Icons.Rounded.QrCodeScanner,
                )
            }

            S5SectionHeader("If pairing fails")
            listOf(
                "Both devices must reach each other — same Wi-Fi, tailnet, or tunnel." to Icons.Rounded.Router,
                "A consumed code can't be reused. Mint a new one with s5 pair." to Icons.Rounded.Lock,
                "Cleartext http:// is allowed for local addresses only." to Icons.Rounded.Lan,
            )
                .forEach { (text, icon) -> S5Notice(icon = icon, text = text) }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}

/**
 * QR pairing.
 *
 * The camera is the fastest path on a phone: the server prints a QR code, and
 * scanning it skips typing a URL with a one-time token in it. Three states show
 * up here and each needs its own answer:
 *
 * - **Permission not yet asked.** Explain why the camera is needed, then ask.
 * - **Permission denied.** Offer the URL form and a route to system settings,
 *   because a second in-app request does nothing once Android has recorded a
 *   denial.
 * - **Scanned.** Parse, then hand host and code back to the form so the user
 *   confirms before the one-time credential is spent — the same review step
 *   RN's scanner leaves in place. A malformed code reports what is wrong and
 *   keeps scanning.
 */
@Composable
fun PairQrScreen(
    onBack: () -> Unit,
    onManual: () -> Unit,
    onScanned: (host: String, code: String) -> Unit,
) {
    val context = LocalContext.current
    var granted by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        )
    }
    var requested by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    var scanned by remember { mutableStateOf(false) }
    val permissionLauncher =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { result ->
            granted = result
            requested = true
        }

    /** One scan: parse, then fill the form. Re-arms on failure so a retry is possible. */
    fun handleScan(value: String) {
        if (scanned) return
        when (val result = parsePairingUrl(extractPairingUrlFromQrPayload(value))) {
            is PairingUrlResult.Invalid -> failure = result.reason.message
            is PairingUrlResult.Valid -> {
                scanned = true
                failure = null
                onScanned(result.target.host, result.target.credential)
            }
        }
    }

    S5Screen(
        title = "Scan pairing code",
        subtitle =
            when {
                scanned -> "Code captured"
                granted -> "Point at the code on your machine"
                else -> "Camera access needed"
            },
        onBack = onBack,
        actions = {
            S5IconButton(
                icon = Icons.Rounded.ContentPaste,
                label = "Enter the URL instead",
                onClick = onManual,
            )
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            if (granted) {
                // The viewfinder is the hero: it takes the space above the notices
                // and carries the expressive corner radius so the camera frame reads
                // as part of the app rather than a raw surface punched into it.
                Box(
                    Modifier.fillMaxWidth()
                        .weight(1f)
                        .padding(horizontal = S5Theme.spacing.gutter)
                        .clip(S5Theme.shapes.largeIncreased)
                        .background(MaterialTheme.colorScheme.surfaceContainerHighest)
                ) {
                    QrScannerPreview(onScanned = ::handleScan, modifier = Modifier.fillMaxSize())
                }
                failure?.let { message ->
                    Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                        S5Notice(
                            icon = Icons.Rounded.Lock,
                            text = message,
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
                        icon = Icons.Rounded.QrCodeScanner,
                        text = "The code is in the terminal output of npx s5 on your machine.",
                    )
                }
            } else {
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Card(tone = S5CardTone.Hero, modifier = Modifier.fillMaxWidth()) {
                        Column(
                            Modifier.fillMaxWidth().padding(S5Theme.spacing.section),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.large),
                        ) {
                            S5ShapeBadge(
                                icon = Icons.Rounded.QrCodeScanner,
                                contentDescription = null,
                                shape = S5MaterialShapes.hero(),
                                containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                                contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                                size = 120.dp,
                                iconSize = 56.dp,
                            )
                            Text(
                                if (requested) "Camera access was denied" else "Scan the pairing code",
                                style = MaterialTheme.typography.titleLargeEmphasized,
                                textAlign = TextAlign.Center,
                            )
                            Text(
                                if (requested) {
                                    "Allow camera access in system settings, or paste the pairing " +
                                        "URL instead. It carries the same one-time token."
                                } else {
                                    "The camera reads the code and never leaves this device. " +
                                        "Nothing is recorded."
                                },
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                textAlign = TextAlign.Center,
                            )
                            if (requested) {
                                S5Button(
                                    text = "Open settings",
                                    onClick = {
                                        context.startActivity(
                                            Intent(
                                                    Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                                                    Uri.fromParts("package", context.packageName, null),
                                                )
                                                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                                        )
                                    },
                                    emphasis = S5ActionEmphasis.Primary,
                                )
                            } else {
                                S5Button(
                                    text = "Allow camera",
                                    onClick = {
                                        permissionLauncher.launch(Manifest.permission.CAMERA)
                                    },
                                    emphasis = S5ActionEmphasis.Primary,
                                    icon = Icons.Rounded.QrCodeScanner,
                                )
                            }
                            S5Button(
                                text = "Enter it manually",
                                onClick = onManual,
                                emphasis = S5ActionEmphasis.Prominent,
                                style = S5ButtonStyle.Outlined,
                                icon = Icons.Rounded.ContentPaste,
                            )
                        }
                    }
                }
            }
        }
    }
}
