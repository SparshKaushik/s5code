package club.touchtech.s5code.kotlin

import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import club.touchtech.s5code.kotlin.data.EnvironmentStore
import club.touchtech.s5code.kotlin.data.LiveWorkspaceGateway
import club.touchtech.s5code.kotlin.transport.EnvironmentHttp
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class StartupTest {
    @Test
    fun workspaceCanStartCollectorsImmediatelyOnMain() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.runOnMainSync {
            val failures = mutableListOf<Throwable>()
            val scope =
                CoroutineScope(
                    SupervisorJob() + Dispatchers.Main.immediate +
                        CoroutineExceptionHandler { _, failure -> failures.add(failure) }
                )
            try {
                val context = instrumentation.targetContext
                val client = EnvironmentHttp.defaultClient()
                val gateway =
                    LiveWorkspaceGateway(
                        context = context,
                        scope = scope,
                        store = EnvironmentStore(context),
                        http = EnvironmentHttp(client),
                        client = client,
                    )
                // A scheduled test dispatcher would hide the constructor's
                // read-before-initialization crash. The app uses Main.immediate.
                assertTrue("Startup collectors failed: $failures", failures.isEmpty())
                assertTrue(gateway.usageLimits.value.pools.isEmpty())
            } finally {
                scope.cancel()
            }
        }
    }

    @Test
    fun launcherActivityReachesResumed() {
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            assertEquals(Lifecycle.State.RESUMED, scenario.state)
        }
    }
}
