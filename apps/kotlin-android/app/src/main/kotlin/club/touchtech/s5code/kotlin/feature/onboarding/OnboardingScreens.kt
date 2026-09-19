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
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.Lan
import androidx.compose.material.icons.rounded.Link
import androidx.compose.material.icons.rounded.Lock
import androidx.compose.material.icons.rounded.QrCodeScanner
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.getValue
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
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5Screen
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

/** The two ways in on first run: direct pairing or an S5 Connect account. */
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
 * Add environment, matching RN's ConnectionsNewRouteScreen: one card with a
 * host field, a pairing-code field, an error banner, and a primary button.
 * The camera icon in the header swaps the card for the scanner, which fills
 * the two fields for confirmation rather than spending the one-time
 * credential sight unseen. Pasting the whole `pairingUrl` line into the host
 * field also works — the code field stays empty and the URL's token is used.
 */
@Composable
fun PairUrlScreen(
    store: AppStore,
    onBack: () -> Unit,
    onPaired: () -> Unit,
    initialHost: String = "",
    initialCode: String = "",
    startScanning: Boolean = false,
) {
    val context = LocalContext.current
    var host by remember { mutableStateOf(initialHost) }
    var code by remember { mutableStateOf(initialCode) }
    var attempted by remember { mutableStateOf(false) }
    var pairing by remember { mutableStateOf(false) }
    // Set only by a rejected exchange. Parse errors come from `result`, and
    // keeping the two apart is what lets editing a field clear one without
    // hiding the other.
    var failure by remember { mutableStateOf<String?>(null) }
    var scanning by remember { mutableStateOf(startScanning) }
    var scanned by remember { mutableStateOf(false) }
    var cameraGranted by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        )
    }
    var cameraRequested by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val cameraLauncher =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            cameraGranted = granted
            cameraRequested = true
        }
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

    /** One scan: parse, fill the fields, close the scanner. Re-arms on failure. */
    fun handleScan(value: String) {
        if (scanned) return
        when (val scanResult = parsePairingUrl(extractPairingUrlFromQrPayload(value))) {
            is PairingUrlResult.Invalid -> failure = scanResult.reason.message
            is PairingUrlResult.Valid -> {
                scanned = true
                failure = null
                host = scanResult.target.host
                code = scanResult.target.credential
                scanning = false
            }
        }
    }

    S5Screen(
        title = if (scanning) "Scan QR code" else "Add environment",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
        actions = {
            S5IconButton(
                icon = if (scanning) Icons.Rounded.Close else Icons.Rounded.QrCodeScanner,
                label = if (scanning) "Close scanner" else "Scan QR code",
                onClick = {
                    if (scanning) {
                        scanning = false
                    } else if (cameraGranted) {
                        scanned = false
                        scanning = true
                    } else {
                        cameraLauncher.launch(Manifest.permission.CAMERA)
                    }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = S5Theme.spacing.gutter),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            if (scanning) {
                if (cameraGranted) {
                    // The viewfinder is square, like RN's CameraView.
                    Box(
                        Modifier.fillMaxWidth()
                            .aspectRatio(1f)
                            .clip(S5Theme.shapes.largeIncreased)
                            .background(MaterialTheme.colorScheme.surfaceContainerHighest)
                    ) {
                        QrScannerPreview(onScanned = ::handleScan, modifier = Modifier.fillMaxSize())
                    }
                } else {
                    S5Card(tone = S5CardTone.Standard, modifier = Modifier.fillMaxWidth()) {
                        Column(
                            Modifier.fillMaxWidth().padding(S5Theme.spacing.xLarge),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                        ) {
                            Text(
                                "Camera permission is required to scan a QR code.",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                textAlign = TextAlign.Center,
                            )
                            if (cameraRequested) {
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
                                    style = S5ButtonStyle.Outlined,
                                )
                            } else {
                                S5Button(
                                    text = "Allow camera",
                                    onClick = {
                                        cameraLauncher.launch(Manifest.permission.CAMERA)
                                    },
                                    emphasis = S5ActionEmphasis.Primary,
                                    icon = Icons.Rounded.QrCodeScanner,
                                )
                            }
                        }
                    }
                }
            } else {
                S5Card(tone = S5CardTone.Standard, modifier = Modifier.fillMaxWidth()) {
                    Column(
                        Modifier.padding(S5Theme.spacing.large),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                    ) {
                        S5TextField(
                            value = host,
                            onValueChange = {
                                host = it
                                attempted = false
                                failure = null
                            },
                            label = "Host",
                            placeholder = "192.168.1.100:8080",
                            isError = attempted && error != null,
                            supporting =
                                when {
                                    attempted && error != null -> error.message
                                    host.contains("://") && code.isBlank() ->
                                        "Pasting the whole pairing URL here works too."
                                    else -> null
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
                            placeholder = "abc-123-xyz",
                            isError = attempted && error != null,
                        )
                        failure?.let { message ->
                            S5Notice(
                                icon = Icons.Rounded.Lock,
                                text = message,
                                containerColor = MaterialTheme.colorScheme.errorContainer,
                                contentColor = MaterialTheme.colorScheme.onErrorContainer,
                            )
                        }
                        S5Button(
                            text = if (pairing) "Pairing…" else "Add environment",
                            onClick = {
                                attempted = true
                                failure = null
                                val target = (result as? PairingUrlResult.Valid)?.target
                                if (target == null) return@S5Button
                                if (pairing) return@S5Button
                                pairing = true
                                scope.launch {
                                    // The credential is one-time, so the exchange writes
                                    // the token before returning; there is nothing to
                                    // retry with this code if it fails.
                                    val outcome = runCatching { store.pairing.pair(target) }
                                    pairing = false
                                    outcome.fold(
                                        onSuccess = { onPaired() },
                                        onFailure = { cause ->
                                            failure =
                                                cause.message ?: "That machine refused the pairing."
                                        },
                                    )
                                }
                            },
                            emphasis = S5ActionEmphasis.Primary,
                            icon = Icons.Rounded.Add,
                            // RN disables only while submitting or with no host —
                            // the missing-code error shows on tap, not up front.
                            enabled = !pairing && host.isNotBlank(),
                        )
                    }
                }
            }
            Box(Modifier.padding(bottom = S5Theme.spacing.section))
        }
    }
}
