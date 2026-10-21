package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ProjectClone
import club.touchtech.s5code.kotlin.transport.wire.ProjectCloneSnapshotDto

fun ProjectCloneSnapshotDto.toModel(): ProjectClone =
    ProjectClone(
        projectId = projectId,
        remoteUrl = remoteUrl,
        destinationPath = destinationPath,
        repositoryName = repository?.nameWithOwner?.takeIf { it.isNotEmpty() },
        phase = phase,
        stage = stage,
        percent = percent?.coerceIn(0, 100),
        detail = detail,
        error = error,
    )
