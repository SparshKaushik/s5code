package club.touchtech.s5code.kotlin.cloud

import club.touchtech.s5code.kotlin.data.EnvironmentStore
import club.touchtech.s5code.kotlin.transport.EnvironmentHttp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** One managed environment as the Connect screen shows it. */
data class CloudEnvironmentRow(
    val environmentId: String,
    val label: String,
    val host: String,
    /** null while the status request is in flight. */
    val online: Boolean?,
    val statusError: String?,
    val linked: Boolean,
)

/** What the managed-environment list is doing. */
sealed interface CloudEnvironmentsState {
    data object Idle : CloudEnvironmentsState

    data object Loading : CloudEnvironmentsState

    data class Loaded(
        val rows: List<CloudEnvironmentRow>,
        /**
         * Other clients signed into this account. Empty is a real answer (a
         * brand-new account, or an account whose only client is this one), which
         * is why the failure lives in [devicesError] instead of in this list.
         */
        val devices: List<CloudDeviceRow> = emptyList(),
        val devicesError: String? = null,
    ) : CloudEnvironmentsState

    data class Failed(val message: String) : CloudEnvironmentsState
}

/**
 * The managed environments a signed-in account can reach, and the act of adding
 * one to this device.
 *
 * Listing and status are two calls with different auth (bearer, then DPoP), which
 * is why the list appears before the health dots do: showing five rows with
 * pending status beats an empty screen while five status probes finish. That
 * matches `cloudEnvironmentsPendingStatus` on mobile.
 *
 * Adding an environment writes a row with no token. Everything else — minting a
 * credential, exchanging it, following a moved tunnel — happens per connection in
 * [RelayEnvironmentAuthorizer], because a credential valid for minutes is useless
 * to persist.
 */
class CloudEnvironments(
    private val scope: CoroutineScope,
    private val relay: RelayClient,
    private val http: EnvironmentHttp,
    private val store: EnvironmentStore,
) {
    private val _state = MutableStateFlow<CloudEnvironmentsState>(CloudEnvironmentsState.Idle)
    val state: StateFlow<CloudEnvironmentsState> = _state.asStateFlow()

    private val _linking = MutableStateFlow<String?>(null)

    /** Environment id currently being added, so its row can show progress. */
    val linking: StateFlow<String?> = _linking.asStateFlow()

    private val _linkError = MutableStateFlow<String?>(null)
    val linkError: StateFlow<String?> = _linkError.asStateFlow()

    fun refresh() {
        scope.launch {
            _state.value = CloudEnvironmentsState.Loading
            val environments =
                try {
                    relay.listEnvironments()
                } catch (error: RelayError) {
                    _state.value = CloudEnvironmentsState.Failed(error.message)
                    return@launch
                }
            val paired = store.environments.value.map { it.environmentId }.toSet()
            _state.value =
                CloudEnvironmentsState.Loaded(
                    environments.map { environment ->
                        CloudEnvironmentRow(
                            environmentId = environment.environmentId,
                            label = environment.label,
                            host = hostOf(environment.endpoint.httpBaseUrl),
                            online = null,
                            statusError = null,
                            linked = environment.environmentId in paired,
                        )
                    }
                )
            // Devices are account metadata, not part of the environment list, so a
            // relay that answers one and not the other still fills the screen. The
            // list is deliberately loaded after the rows are on screen for the same
            // reason status is.
            scope.launch {
                val result = runCatching { relay.listDevices() }
                updateLoaded { loaded ->
                    result.fold(
                        onSuccess = {
                            loaded.copy(
                                devices = CloudDevicePresentation.rows(it),
                                devicesError = null,
                            )
                        },
                        onFailure = { cause ->
                            loaded.copy(
                                devices = emptyList(),
                                devicesError =
                                    (cause as? RelayError)?.message
                                        ?: "Could not list your other devices.",
                            )
                        },
                    )
                }
            }
            // Status probes are independent: one sleeping machine must not hold up
            // the health of the others, and each row updates as its answer lands.
            environments.forEach { environment ->
                scope.launch {
                    val result = runCatching { relay.environmentStatus(environment.environmentId) }
                    updateRow(environment.environmentId) { row ->
                        result.fold(
                            onSuccess = { status ->
                                row.copy(online = status.status == "online", statusError = status.error)
                            },
                            onFailure = { cause ->
                                row.copy(
                                    online = false,
                                    statusError = (cause as? RelayError)?.message ?: "Status unavailable.",
                                )
                            },
                        )
                    }
                }
            }
        }
    }

    /**
     * Adds one managed environment to this device.
     *
     * The descriptor read is what makes the row honest: the relay knows a label,
     * but the environment knows its own id, platform, and version, and a row built
     * from the relay's view alone would disagree with the connections screen the
     * moment the machine is renamed.
     */
    fun link(environmentId: String, onLinked: () -> Unit) {
        if (_linking.value != null) return
        scope.launch {
            _linking.value = environmentId
            _linkError.value = null
            try {
                val connect = relay.connectEnvironment(environmentId, deviceId = null)
                val descriptor = http.descriptor(connect.endpoint.httpBaseUrl)
                store.saveManaged(
                    environmentId = descriptor.environmentId,
                    label = descriptor.label,
                    httpBaseUrl = connect.endpoint.httpBaseUrl,
                    wsBaseUrl = connect.endpoint.wsBaseUrl,
                    platform = descriptor.platform.display,
                    serverVersion = descriptor.serverVersion.orEmpty(),
                )
                updateRow(environmentId) { it.copy(linked = true) }
                onLinked()
            } catch (cause: Exception) {
                _linkError.value = cause.message ?: "Could not add that machine."
            } finally {
                _linking.value = null
            }
        }
    }

    fun clearLinkError() {
        _linkError.value = null
    }

    private fun updateRow(environmentId: String, transform: (CloudEnvironmentRow) -> CloudEnvironmentRow) {
        updateLoaded { loaded ->
            loaded.copy(
                rows = loaded.rows.map { if (it.environmentId == environmentId) transform(it) else it }
            )
        }
    }

    /**
     * Applies [transform] only while the list is loaded. A probe that lands after
     * a refresh moved the screen back to Loading must be dropped, not resurrect a
     * stale list.
     */
    private fun updateLoaded(
        transform: (CloudEnvironmentsState.Loaded) -> CloudEnvironmentsState.Loaded
    ) {
        val current = _state.value as? CloudEnvironmentsState.Loaded ?: return
        _state.value = transform(current)
    }

    private fun hostOf(httpBaseUrl: String): String =
        httpBaseUrl.removePrefix("https://").removePrefix("http://").trimEnd('/')
}
