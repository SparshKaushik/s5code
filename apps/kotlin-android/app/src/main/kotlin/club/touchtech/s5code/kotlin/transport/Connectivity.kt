package club.touchtech.s5code.kotlin.transport

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.distinctUntilChanged

/**
 * Default-network reachability, fed to the session supervisor.
 *
 * `NET_CAPABILITY_VALIDATED` is the answer that matters: a captive portal or a
 * DNS-blackholed cell link still reports INTERNET, and a supervisor that trusted
 * it would burn its backoff ladder failing sockets that could never open.
 * Before the first callback the flow assumes online — optimistic is right for a
 * monitor that exists to *avoid* burning attempts, not to gate them.
 */
object Connectivity {
    fun online(context: Context): Flow<Boolean> =
        callbackFlow {
                val manager = context.getSystemService(ConnectivityManager::class.java)
                if (manager == null) {
                    trySend(true)
                    close()
                    return@callbackFlow
                }
                val callback =
                    object : ConnectivityManager.NetworkCallback() {
                        override fun onAvailable(network: Network) {
                            trySend(validated(manager, network))
                        }

                        override fun onLost(network: Network) {
                            trySend(validated(manager, manager.activeNetwork))
                        }

                        override fun onCapabilitiesChanged(
                            network: Network,
                            capabilities: NetworkCapabilities,
                        ) {
                            if (network == manager.activeNetwork) {
                                trySend(
                                    capabilities.hasCapability(
                                        NetworkCapabilities.NET_CAPABILITY_VALIDATED
                                    )
                                )
                            }
                        }
                    }
                trySend(validated(manager, manager.activeNetwork))
                manager.registerDefaultNetworkCallback(callback)
                awaitClose { manager.unregisterNetworkCallback(callback) }
            }
            .distinctUntilChanged()
            .conflate()

    private fun validated(manager: ConnectivityManager, network: Network?): Boolean {
        if (network == null) return false
        val capabilities = manager.getNetworkCapabilities(network) ?: return false
        return capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
    }
}
