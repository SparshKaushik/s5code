package club.touchtech.s5code.kotlin.feature.home

import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.Environment
import club.touchtech.s5code.kotlin.model.EnvironmentDevice
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.EnvironmentKind
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProjectId
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.PullRequestRef
import club.touchtech.s5code.kotlin.model.PullRequestState
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSummary

/**
 * Home-list fixture: three environments, four projects, and one thread per
 * status the list has to lay out.
 *
 * It lives in the test source set on purpose. The projection is pure and its
 * layout rules (shelves, grouping, attention ordering, the environment label)
 * only show up on data with that shape, so the shape is pinned here rather than
 * shipped in the app.
 */
internal object HomeFixture {

    /**
     * Fixed clock for the fixture. Sorting is on an instant, so the fixture needs
     * one; deriving it from `System.currentTimeMillis()` would make ordering
     * assertions depend on when the test ran.
     */
    private const val NOW = 1_760_000_000_000L

    val laptop = EnvironmentId("env-macbook")
    val devbox = EnvironmentId("env-devbox")
    val cloud = EnvironmentId("env-cloud")

    val environments =
        listOf(
            Environment(
                id = laptop,
                label = "MacBook Pro",
                host = "http://macbook.tail9c2f.ts.net:4488",
                kind = EnvironmentKind.Direct,
                state = ConnectionState.Connected,
                lastSeenLabel = "now",
                devices =
                    listOf(
                        EnvironmentDevice("MacBook Pro", "macOS 26.1", true, "now"),
                        EnvironmentDevice("Pixel 9 Pro", "Android 16", true, "2m ago"),
                    ),
            ),
            Environment(
                id = devbox,
                label = "devbox-01",
                host = "http://10.0.4.21:4488",
                kind = EnvironmentKind.Direct,
                state = ConnectionState.Recovering,
                lastSeenLabel = "12s ago",
                devices = listOf(EnvironmentDevice("devbox-01", "Ubuntu 24.04", true, "12s ago")),
            ),
            Environment(
                id = cloud,
                label = "S5 Connect · sandbox",
                host = "relay.s5code.dpdns.org",
                kind = EnvironmentKind.Cloud,
                state = ConnectionState.Offline,
                lastSeenLabel = "1h ago",
                devices = listOf(EnvironmentDevice("sandbox-runner", "Linux", false, "1h ago")),
            ),
        )

    val projects =
        listOf(
            Project(
                id = ProjectId("proj-t3code"),
                environmentId = laptop,
                title = "s5code",
                workspaceRoot = "~/code/s5code",
                repository = "touchtech/s5code",
                branch = "main",
            ),
            Project(
                id = ProjectId("proj-relay"),
                environmentId = laptop,
                title = "s5-relay",
                workspaceRoot = "~/code/s5-relay",
                repository = "touchtech/s5-relay",
                branch = "main",
            ),
            Project(
                id = ProjectId("proj-marketing"),
                environmentId = devbox,
                title = "marketing-site",
                workspaceRoot = "/srv/marketing-site",
                repository = "touchtech/marketing-site",
                branch = "develop",
            ),
            Project(
                id = ProjectId("proj-sandbox"),
                environmentId = cloud,
                title = "playground",
                workspaceRoot = "/workspace/playground",
                repository = null,
                branch = "main",
            ),
        )

    val threads =
        listOf(
            ThreadSummary(
                id = ThreadId("thr-approval"),
                environmentId = laptop,
                projectId = ProjectId("proj-t3code"),
                title = "Rebuild the mobile client in Kotlin with Material 3 Expressive",
                status = ThreadStatus.AwaitingApproval,
                provider = ProviderInstance("codex", "codex"),
                model = "gpt-5-codex",
                branch = "kotlin-android-client",
                updatedLabel = "now",
                updatedAtMillis = NOW - 0 * 60_000L,
                pinned = true,
                changedFiles = 34,
                additions = 4820,
                deletions = 112,
                elapsedLabel = "6m 12s",
                excerpt = "Waiting on approval to run ./gradlew :app:assembleRelease",
            ),
            ThreadSummary(
                id = ThreadId("thr-working"),
                environmentId = laptop,
                projectId = ProjectId("proj-t3code"),
                title = "Cut websocket payload size on the home stream",
                status = ThreadStatus.Working,
                provider = ProviderInstance("claudeAgent", "claudeAgent"),
                model = "claude-sonnet-4.6",
                branch = "perf/home-stream",
                updatedLabel = "now",
                updatedAtMillis = NOW - 0 * 60_000L,
                changedFiles = 6,
                additions = 214,
                deletions = 96,
                elapsedLabel = "1m 04s",
                excerpt = "Reading packages/contracts/src/rpc.ts",
            ),
            ThreadSummary(
                id = ThreadId("thr-input"),
                environmentId = devbox,
                projectId = ProjectId("proj-marketing"),
                title = "Refresh the pricing page hero",
                status = ThreadStatus.AwaitingInput,
                provider = ProviderInstance("cursor", "cursor"),
                model = "composer-1",
                branch = "marketing/pricing-hero",
                updatedLabel = "3m ago",
                updatedAtMillis = NOW - 3 * 60_000L,
                changedFiles = 3,
                additions = 88,
                deletions = 41,
                excerpt = "Which headline should ship?",
            ),
            ThreadSummary(
                id = ThreadId("thr-failed"),
                environmentId = devbox,
                projectId = ProjectId("proj-marketing"),
                title = "Migrate the blog to the new MDX pipeline",
                status = ThreadStatus.Failed,
                provider = ProviderInstance("grok", "grok"),
                model = "grok-code-fast",
                branch = "marketing/mdx",
                updatedLabel = "18m ago",
                updatedAtMillis = NOW - 18 * 60_000L,
                lastError = "pnpm build failed: Cannot find module '@mdx-js/rollup'",
                changedFiles = 12,
                additions = 402,
                deletions = 380,
            ),
            ThreadSummary(
                id = ThreadId("thr-pr"),
                environmentId = laptop,
                projectId = ProjectId("proj-relay"),
                title = "Normalize escaped FCM private keys",
                status = ThreadStatus.Idle,
                provider = ProviderInstance("codex", "codex"),
                model = "gpt-5-codex",
                branch = "fix/fcm-keys",
                updatedLabel = "42m ago",
                updatedAtMillis = NOW - 42 * 60_000L,
                pullRequest = PullRequestRef(318, PullRequestState.Open, "fix(relay): normalize escaped FCM keys"),
                changedFiles = 2,
                additions = 47,
                deletions = 9,
            ),
            ThreadSummary(
                id = ThreadId("thr-queued"),
                environmentId = cloud,
                projectId = ProjectId("proj-sandbox"),
                title = "Try the new plan mode against a scratch repo",
                status = ThreadStatus.Queued,
                provider = ProviderInstance("opencode", "opencode"),
                model = "qwen3-coder",
                branch = null,
                updatedLabel = "queued",
                updatedAtMillis = NOW - 5 * 60_000L,
                excerpt = "Waiting for S5 Connect · sandbox to come online",
            ),
            ThreadSummary(
                id = ThreadId("thr-snoozed"),
                environmentId = laptop,
                projectId = ProjectId("proj-relay"),
                title = "Audit relay token rotation for multi-device",
                status = ThreadStatus.Snoozed,
                provider = ProviderInstance("claudeAgent", "claudeAgent"),
                model = "claude-opus-4.2",
                branch = "relay/token-audit",
                updatedLabel = "2h ago",
                updatedAtMillis = NOW - 120 * 60_000L,
                snoozedUntilLabel = "tomorrow, 09:00",
            ),
            ThreadSummary(
                id = ThreadId("thr-settled-1"),
                environmentId = laptop,
                projectId = ProjectId("proj-t3code"),
                title = "Preserve Android live update order",
                status = ThreadStatus.Settled,
                provider = ProviderInstance("codex", "codex"),
                model = "gpt-5-codex",
                branch = "fix/live-update-order",
                updatedLabel = "yesterday",
                updatedAtMillis = NOW - 1440 * 60_000L,
                pullRequest = PullRequestRef(311, PullRequestState.Merged, "fix(relay): preserve live update order"),
                changedFiles = 4,
                additions = 96,
                deletions = 34,
            ),
            ThreadSummary(
                id = ThreadId("thr-settled-2"),
                environmentId = devbox,
                projectId = ProjectId("proj-marketing"),
                title = "Ship the changelog route",
                status = ThreadStatus.Settled,
                provider = ProviderInstance("cursor", "cursor"),
                model = "composer-1",
                branch = "marketing/changelog",
                updatedLabel = "2d ago",
                updatedAtMillis = NOW - 2880 * 60_000L,
                pullRequest = PullRequestRef(64, PullRequestState.Merged, "feat(site): changelog route"),
            ),
            ThreadSummary(
                id = ThreadId("thr-settled-3"),
                environmentId = laptop,
                projectId = ProjectId("proj-relay"),
                title = "Make the FCM migration idempotent",
                status = ThreadStatus.Settled,
                provider = ProviderInstance("codex", "codex"),
                model = "gpt-5-codex",
                branch = "fix/fcm-idempotent",
                updatedLabel = "3d ago",
                updatedAtMillis = NOW - 4320 * 60_000L,
            ),
        )
}
