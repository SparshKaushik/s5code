package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.PullRequestChangeBreakdownDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestLineCountDto
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Test

/** Port of the shared `changedFileKind` tests, plus the files screen's breakdown line. */
class ChangedFileKindTest {

    @Test
    fun `classifies changed files by path`() {
        val cases =
            listOf(
                "apps/server/src/ws.ts" to ChangedFileKind.Code,
                "src/latest.ts" to ChangedFileKind.Code,
                "src/Latest.java" to ChangedFileKind.Code,
                "src/contest.py" to ChangedFileKind.Code,
                "src/mockingbird.ts" to ChangedFileKind.Code,
                "src/testimonials/List.tsx" to ChangedFileKind.Code,
                "apps/web/src/foo.test.ts" to ChangedFileKind.Tests,
                "apps/web/src/Foo.browser.test.tsx" to ChangedFileKind.Tests,
                "src/foo.spec.js" to ChangedFileKind.Tests,
                "pkg/server_test.go" to ChangedFileKind.Tests,
                "tests/test_api.py" to ChangedFileKind.Tests,
                "conftest.py" to ChangedFileKind.Tests,
                "src/main/java/RequestTest.java" to ChangedFileKind.Tests,
                "Sources/AppTests.swift" to ChangedFileKind.Tests,
                "src/HTTPTest.java" to ChangedFileKind.Tests,
                "Sources/URLTests.swift" to ChangedFileKind.Tests,
                "spec/models/user_spec.rb" to ChangedFileKind.Tests,
                "src/__tests__/helpers.ts" to ChangedFileKind.Tests,
                "src/testUtils.ts" to ChangedFileKind.Code,
                "src\\__tests__\\app.ts" to ChangedFileKind.Tests,
                "src/__mocks__/fs.ts" to ChangedFileKind.Mocks,
                "test/fixtures/pr.json" to ChangedFileKind.Mocks,
                "src/__snapshots__/view.test.ts.snap" to ChangedFileKind.Mocks,
                "src/api.mock.ts" to ChangedFileKind.Mocks,
                "src/mockServer.ts" to ChangedFileKind.Mocks,
                "pkg/testdata/input.txt" to ChangedFileKind.Mocks,
                "pnpm-lock.yaml" to ChangedFileKind.Generated,
                "apps/web/package-lock.json" to ChangedFileKind.Generated,
                "go.sum" to ChangedFileKind.Generated,
                "src/routeTree.gen.ts" to ChangedFileKind.Generated,
                "src/__generated__/types.ts" to ChangedFileKind.Generated,
                "dist/app.min.js" to ChangedFileKind.Generated,
                "README.md" to ChangedFileKind.Docs,
                "docs/internals/overview.mdx" to ChangedFileKind.Docs,
                "NOTES.txt" to ChangedFileKind.Docs,
                "requirements-dev.txt" to ChangedFileKind.Code,
                "CMakeLists.txt" to ChangedFileKind.Code,
                "tests/README.md" to ChangedFileKind.Tests,
            )
        for ((path, kind) in cases) assertEquals(path, kind, classifyChangedFile(path))
    }

    @Test
    fun `lists kinds in order with grouped counts, skipping unchanged ones`() {
        val breakdown =
            PullRequestChangeBreakdownDto(
                code = PullRequestLineCountDto(1234, 56),
                tests = PullRequestLineCountDto(8, 0),
                generated = PullRequestLineCountDto(0, 2),
            )
        assertEquals(
            "Code +1,234 −56 · Tests +8 −0 · Generated +0 −2",
            pullRequestChangeBreakdownLabel(breakdown, Locale.US),
        )
        assertEquals("", pullRequestChangeBreakdownLabel(PullRequestChangeBreakdownDto(), Locale.US))
    }
}
