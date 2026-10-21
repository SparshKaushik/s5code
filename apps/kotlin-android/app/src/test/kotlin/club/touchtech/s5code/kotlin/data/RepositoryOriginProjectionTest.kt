package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.feature.home.buildProjectScopes
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ProjectGrouping
import club.touchtech.s5code.kotlin.transport.TransportJson
import club.touchtech.s5code.kotlin.transport.wire.ProjectShellDto
import org.junit.Assert.assertEquals
import org.junit.Test

class RepositoryOriginProjectionTest {
    @Test
    fun `decoded fork origin keeps project separate from upstream across environments`() {
        val wire = """{
            "id":"project-1","title":"Repo","workspaceRoot":"/workspace/repo",
            "repositoryIdentity":{
                "canonicalKey":"github.com/upstream/repo","displayName":"upstream/repo",
                "origin":{"canonicalKey":"github.com/fork/repo","displayName":"fork/repo"}
            }
        }"""
        val dto = TransportJson.decodeFromString(ProjectShellDto.serializer(), wire)
        val fork = projectFrom(EnvironmentId("env-1"), dto)
        val upstream = projectFrom(EnvironmentId("env-2"), dto.copy(
            repositoryIdentity = dto.repositoryIdentity!!.copy(origin = null),
        ))
        assertEquals("fork/repo", fork.repository)
        assertEquals("github.com/fork/repo", fork.repositoryIdentity!!.groupingCanonicalKey)
        assertEquals("github.com/upstream/repo", fork.repositoryIdentity!!.canonicalKey)
        assertEquals(2, buildProjectScopes(listOf(fork, upstream), ProjectGrouping.Repository).size)
    }
}
