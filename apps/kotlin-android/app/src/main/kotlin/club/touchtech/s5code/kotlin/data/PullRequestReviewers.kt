package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.PullRequestActorDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewerCandidateDto

/* Review requests, ported from RN's `PullRequestReviewersScreen` and the
 * `requestReviewers` cache patches in client-runtime `state/pullRequests.ts`. */

/** Candidates whose login or name contains [query]; requested first, then by login. */
fun filterReviewerCandidates(candidates: List<PullRequestReviewerCandidateDto>, query: String): List<PullRequestReviewerCandidateDto> {
    val needle = query.trim().lowercase()
    return candidates
        .filter { "${it.login} ${it.name.orEmpty()}".lowercase().contains(needle) }
        .sortedWith(compareByDescending<PullRequestReviewerCandidateDto> { it.isRequested }.thenBy { it.login })
}

/** The cached candidates a request names, matched on id and kind. */
fun selectedReviewerCandidates(
    candidates: List<PullRequestReviewerCandidateDto>,
    reviewers: List<PullRequestReviewerRequest>,
): List<PullRequestReviewerCandidateDto> =
    candidates.filter { candidate -> reviewers.any { it.id == candidate.id && it.kind == candidate.kind } }

/** Marks [selected] as (un)requested in the candidate list. */
fun patchReviewerCandidates(
    candidates: List<PullRequestReviewerCandidateDto>,
    selected: List<PullRequestReviewerCandidateDto>,
    requested: Boolean,
): List<PullRequestReviewerCandidateDto> =
    candidates.map { candidate -> if (candidate in selected) candidate.copy(isRequested = requested) else candidate }

/**
 * A PR's reviewer list after (un)requesting [selected]: requesting appends the
 * ones not already listed, unrequesting drops them unless [keep] holds.
 */
fun patchRequestedReviewers(
    actors: List<PullRequestActorDto>,
    selected: List<PullRequestReviewerCandidateDto>,
    requested: Boolean,
    keep: (PullRequestActorDto) -> Boolean = { false },
): List<PullRequestActorDto> {
    if (requested) {
        return actors +
            selected
                .filter { candidate -> actors.none { it.login.equals(candidate.login, ignoreCase = true) } }
                .map { it.actor }
    }
    val logins = selected.mapTo(HashSet()) { it.login.lowercase() }
    return actors.filter { it.login.lowercase() !in logins || keep(it) }
}
