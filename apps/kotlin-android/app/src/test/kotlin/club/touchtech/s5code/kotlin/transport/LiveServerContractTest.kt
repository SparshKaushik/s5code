package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.data.DEFAULT_TERMINAL_ID
import club.touchtech.s5code.kotlin.transport.wire.FilesystemBrowseResultDto
import club.touchtech.s5code.kotlin.transport.wire.ProjectListEntriesResultDto
import club.touchtech.s5code.kotlin.transport.wire.ReviewDiffPreviewResultDto
import club.touchtech.s5code.kotlin.transport.wire.ServerConfigDto
import club.touchtech.s5code.kotlin.transport.wire.ShellStreamItemDto
import club.touchtech.s5code.kotlin.transport.wire.TerminalStreamEventDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadStreamItemDto
import club.touchtech.s5code.kotlin.transport.wire.UsageSummaryDto
import club.touchtech.s5code.kotlin.transport.wire.VcsListRefsResultDto
import club.touchtech.s5code.kotlin.transport.wire.VcsStatusDto
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import org.junit.Assume.assumeTrue
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Decodes every payload this client reads, against a real running server.
 *
 * This exists because the unit tests decode fixtures *this repo wrote*, and a
 * hand-written DTO that disagrees with the contract passes all of them while
 * failing on the first real response. Two bugs shipped that way: a DPoP header
 * missing its fixed JWK members, and `platform` typed as a string when the
 * contract has it as a struct. Both were invisible until a device tried.
 *
 * Skipped unless `T3_LIVE_HTTP` and `T3_LIVE_TOKEN` are set, so it never fails a
 * normal run. Point it at a disposable server, never the developer's live one:
 *
 * ```
 * node apps/server/src/bin.ts serve --port 39311 --base-dir "$PWD/.t3"
 * T3_LIVE_HTTP=http://127.0.0.1:39311 T3_LIVE_TOKEN=<pairing token> \
 *   ./gradlew :app:testDebugUnitTest --tests '*LiveServerContractTest*'
 * ```
 */
class LiveServerContractTest {

    private val httpBaseUrl = System.getenv("T3_LIVE_HTTP")
    private val pairingToken = System.getenv("T3_LIVE_TOKEN")

    private val http = EnvironmentHttp(EnvironmentHttp.defaultClient())

    @Test
    // `runBlocking`, not `runTest`: the RPC connect has a real timeout, and
    // `runTest`'s virtual clock fires it before the socket can open.
    fun `every payload this client reads decodes against a real server`() = runBlocking {
        assumeTrue(!httpBaseUrl.isNullOrBlank() && !pairingToken.isNullOrBlank())
        val base = httpBaseUrl!!

        // 1. Descriptor. The unauthenticated identity read, and the one that was
        // wrong: `platform` is a struct, not a string.
        val descriptor = http.descriptor(base)
        assertTrue(descriptor.environmentId.isNotBlank())
        assertTrue(descriptor.label.isNotBlank())
        assertTrue(
            "platform must decode as a struct, not a string",
            descriptor.platform.os.isNotBlank(),
        )
        assertTrue(descriptor.platform.display.contains("/"))

        // 2. Token exchange. Single-use, so this test cannot be re-run against the
        // same token; mint a fresh one per run.
        val access =
            http.exchangePairingCredential(
                httpBaseUrl = base,
                credential = pairingToken!!,
                deviceLabel = "Kotlin contract test",
            )
        assertTrue(access.access_token.isNotBlank())
        assertEquals("Bearer", access.token_type)
        val credential = EnvironmentCredential.Bearer(access.access_token)

        // 3. Session and socket ticket.
        assertTrue(http.session(base, credential).authenticated)
        val socketUrl = http.resolveSocketUrl(base, base.replaceFirst("http", "ws"), credential)
        assertTrue(socketUrl.contains("/ws?wsTicket="))

        val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        val connection = RpcConnection.open(EnvironmentHttp.defaultClient(), socketUrl, scope)
        try {
            // 4. server.getConfig, the largest payload the client decodes and the
            // one that carries the same platform struct plus every provider.
            val config =
                withTimeout(TIMEOUT) {
                    connection.call(
                        WsMethods.ServerGetConfig,
                        JsonObject(emptyMap()),
                        ServerConfigDto.serializer(),
                    )
                }
            assertEquals(descriptor.environmentId, config.environment.environmentId)
            assertTrue(config.environment.platform.os.isNotBlank())
            assertTrue(config.cwd.isNotBlank())

            // 5. The shell snapshot: projects and threads, the home list's model.
            val shell =
                withTimeout(TIMEOUT) {
                    connection
                        .stream(WsMethods.OrchestrationSubscribeShell, JsonObject(emptyMap()))
                        .first()
                }
            val frame = TransportJson.decodeFromJsonElement(ShellStreamItemDto.serializer(), shell)
            assertEquals("snapshot", frame.kind)
            val snapshot = frame.snapshot!!
            // Seeded state, not an empty database: a decoder that silently drops
            // rows would pass every assertion below on zero items.
            assertTrue("seed the worktree .t3 first", snapshot.projects.isNotEmpty())
            assertTrue("seed the worktree .t3 first", snapshot.threads.isNotEmpty())
            val project = snapshot.projects.first()
            assertTrue(project.workspaceRoot.isNotBlank())

            // 6. Thread detail snapshots, the largest and most provider-shaped
            // payload the client reads. Several threads are walked so the sweep
            // covers a real spread of activity kinds, and at least one must carry
            // messages: a decoder that dropped them would otherwise pass on empty
            // lists.
            var decodedMessages = 0
            var decodedActivities = 0
            for (thread in snapshot.threads.take(THREAD_SAMPLE)) {
                val detail =
                    withTimeout(TIMEOUT) {
                        connection
                            .stream(
                                WsMethods.OrchestrationSubscribeThread,
                                buildJsonObject { put("threadId", JsonPrimitive(thread.id)) },
                            )
                            .first()
                    }
                val threadFrame =
                    TransportJson.decodeFromJsonElement(ThreadStreamItemDto.serializer(), detail)
                assertEquals("snapshot", threadFrame.kind)
                val decoded = threadFrame.snapshot!!.thread
                assertEquals(thread.id, decoded.id)
                decodedMessages += decoded.messages.size
                decodedActivities += decoded.activities.size
            }
            assertTrue("no messages decoded from any thread", decodedMessages > 0)
            assertTrue("no activities decoded from any thread", decodedActivities > 0)

            // 7. The workspace tool reads, all addressed by cwd rather than thread.
            // `config.cwd` rather than a project's `workspaceRoot`: seeded rows
            // point at directories from the machine the snapshot was copied from,
            // and review rejects a cwd outside the server's own workspace root.
            val cwd = config.cwd
            val entries =
                withTimeout(TIMEOUT) {
                    connection.call(
                        WsMethods.ProjectsListEntries,
                        buildJsonObject { put("cwd", JsonPrimitive(cwd)) },
                        ProjectListEntriesResultDto.serializer(),
                    )
                }
            assertTrue(entries.entries.isNotEmpty())

            val status =
                withTimeout(TIMEOUT) {
                    connection.call(
                        WsMethods.VcsRefreshStatus,
                        buildJsonObject { put("cwd", JsonPrimitive(cwd)) },
                        VcsStatusDto.serializer(),
                    )
                }
            assertTrue(status.isRepo)

            val refs =
                withTimeout(TIMEOUT) {
                    connection.call(
                        WsMethods.VcsListRefs,
                        buildJsonObject {
                            put("cwd", JsonPrimitive(cwd))
                            put("refKind", JsonPrimitive("local"))
                            put("limit", JsonPrimitive(100))
                        },
                        VcsListRefsResultDto.serializer(),
                    )
                }
            assertTrue(refs.refs.isNotEmpty())

            withTimeout(TIMEOUT) {
                connection.call(
                    WsMethods.ReviewGetDiffPreview,
                    buildJsonObject { put("cwd", JsonPrimitive(cwd)) },
                    ReviewDiffPreviewResultDto.serializer(),
                )
            }

            withTimeout(TIMEOUT) {
                connection.call(
                    WsMethods.FilesystemBrowse,
                    buildJsonObject { put("partialPath", JsonPrimitive("~/")) },
                    FilesystemBrowseResultDto.serializer(),
                )
            }

            // Usage takes a required day window and time zone, the same shape the
            // gateway sends.
            withTimeout(TIMEOUT) {
                connection.call(
                    WsMethods.ServerGetUsageSummary,
                    buildJsonObject {
                        put("sinceDay", JsonPrimitive("2026-08-01"))
                        put("untilDay", JsonPrimitive("2026-08-31"))
                        put("timeZone", JsonPrimitive("UTC"))
                    },
                    UsageSummaryDto.serializer(),
                )
            }

            // 8. Terminal attach, which spawns a shell on the server. Safe here
            // because this runs against a disposable environment, and it is the only
            // way to prove the streaming path the terminal screen actually uses:
            // `attach` opens or reattaches and pushes a snapshot frame first.
            val threadId = snapshot.threads.first().id
            val attachFrame =
                withTimeout(TIMEOUT) {
                    connection
                        .stream(
                            WsMethods.TerminalAttach,
                            buildJsonObject {
                                put("threadId", JsonPrimitive(threadId))
                                put("terminalId", JsonPrimitive(DEFAULT_TERMINAL_ID))
                                put("cwd", JsonPrimitive(cwd))
                                put("cols", JsonPrimitive(80))
                                put("rows", JsonPrimitive(24))
                                put("restartIfNotRunning", JsonPrimitive(true))
                            },
                        )
                        .first()
                }
            val attach =
                TransportJson.decodeFromJsonElement(
                    TerminalStreamEventDto.serializer(),
                    attachFrame,
                )
            assertEquals("snapshot", attach.type)
            assertEquals(DEFAULT_TERMINAL_ID, attach.snapshot!!.terminalId)

            // `terminal.write` declares no success value, so Effect omits `value`
            // from the exit. Decoding it into a struct is what broke writes before;
            // this proves the raw request path works.
            withTimeout(TIMEOUT) {
                connection.request(
                    WsMethods.TerminalWrite,
                    buildJsonObject {
                        put("threadId", JsonPrimitive(threadId))
                        put("terminalId", JsonPrimitive(DEFAULT_TERMINAL_ID))
                        put("data", JsonPrimitive("true\r"))
                    },
                )
            }
            withTimeout(TIMEOUT) {
                connection.request(
                    WsMethods.TerminalResize,
                    buildJsonObject {
                        put("threadId", JsonPrimitive(threadId))
                        put("terminalId", JsonPrimitive(DEFAULT_TERMINAL_ID))
                        put("cols", JsonPrimitive(100))
                        put("rows", JsonPrimitive(30))
                    },
                )
            }
            withTimeout(TIMEOUT) {
                connection.request(
                    WsMethods.TerminalClose,
                    buildJsonObject {
                        put("threadId", JsonPrimitive(threadId))
                        put("terminalId", JsonPrimitive(DEFAULT_TERMINAL_ID))
                    },
                )
            }
        } finally {
            connection.close()
            scope.cancel()
        }
        Unit
    }

    private companion object {
        const val TIMEOUT = 30_000L

        /** How many threads to walk. Enough for a real spread, quick enough to run. */
        const val THREAD_SAMPLE = 5
    }
}
