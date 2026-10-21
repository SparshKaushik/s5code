package club.touchtech.s5code.kotlin.transport

/** `ORCHESTRATION_PROTOCOL_VERSION`: the orchestration wire protocol this client speaks. */
const val ORCHESTRATION_PROTOCOL_VERSION = 2

/**
 * `orchestrationProtocolCompatibilityError` from
 * `packages/client-runtime/src/connection/compatibility.ts`: the detail to show
 * when the server speaks another orchestration protocol, or null when it is
 * compatible. Servers shipped before negotiation omit the version and speak the
 * original protocol, 1. The message says which side needs updating.
 */
fun orchestrationProtocolCompatibilityError(serverProtocolVersion: Int?, label: String): String? {
    val version = serverProtocolVersion ?: 1
    if (version == ORCHESTRATION_PROTOCOL_VERSION) return null
    return if (version > ORCHESTRATION_PROTOCOL_VERSION) {
        "This client is not supported by this server. Update your app or use a compatible release to connect to $label."
    } else {
        "This client requires a newer server. Update S5 Code on $label to connect."
    }
}

/**
 * `serverUpdateRequired` in RN: an outdated server that can update itself is
 * still saved at pairing, so the host can be updated and reconnected.
 */
fun pairingBlockedByProtocol(descriptor: EnvironmentDescriptorDto): String? {
    val detail =
        orchestrationProtocolCompatibilityError(descriptor.orchestrationProtocolVersion, descriptor.label)
            ?: return null
    val outdated = (descriptor.orchestrationProtocolVersion ?: 1) < ORCHESTRATION_PROTOCOL_VERSION
    val selfUpdate = descriptor.capabilities.serverSelfUpdate
    val canSelfUpdate =
        selfUpdate != null && (selfUpdate != "desktop-managed" || descriptor.capabilities.desktopAppUpdate)
    return if (outdated && canSelfUpdate) null else detail
}

/** The server speaks an orchestration protocol this client cannot; retrying will not help. */
class EnvironmentUnsupportedError(message: String) : Exception(message)
