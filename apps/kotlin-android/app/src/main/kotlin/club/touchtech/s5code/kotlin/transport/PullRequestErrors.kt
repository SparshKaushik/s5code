package club.touchtech.s5code.kotlin.transport

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/*
 * The text `PullRequestUnavailableError.message` and
 * `PullRequestOperationError.message` build in
 * `packages/contracts/src/pullRequest.ts`. Those getters run on the client, so
 * the wire carries only `reason`/`provider`/`operation`/`detail` and the words
 * have to be rebuilt here.
 */

private class ProviderRequirement(val missing: String, val unauthenticated: String)

private val PROVIDER_REQUIREMENT =
    mapOf(
        "github" to
            ProviderRequirement(
                missing =
                    "No GitHub credential on the server. Set GH_TOKEN, or install the GitHub CLI (https://cli.github.com/) and run `gh auth login`.",
                unauthenticated =
                    "GitHub has no working credential for this host. Run `gh auth login`, or check the account and hosts in Settings → Source Control.",
            ),
        "forgejo" to
            ProviderRequirement(
                missing =
                    "Install Forgejo CLI (`fj` 0.6 or later) from https://codeberg.org/forgejo-contrib/forgejo-cli or Gitea CLI (`tea` 0.16 or later) from https://gitea.com/gitea/tea to browse Forgejo pull requests.",
                unauthenticated =
                    "Authenticate your Forgejo or Gitea server with `fj --host <server-url> auth add-token` on the S5 Code server. If fj is missing or unconfigured for that server, use `tea login add`. A configured fj account must be repaired with fj.",
            ),
        "gitlab" to
            ProviderRequirement(
                missing =
                    "GitLab CLI (`glab`) is required to browse change requests on this host. Install it from https://gitlab.com/gitlab-org/cli and reload.",
                unauthenticated = "GitLab CLI is not authenticated. Run `glab auth login` and retry.",
            ),
        "azure-devops" to
            ProviderRequirement(
                missing =
                    "Azure CLI (`az`) with the Azure DevOps extension is required. Install `az`, then run `az extension add --name azure-devops`.",
                unauthenticated = "Azure CLI is not signed in. Run `az login` and retry.",
            ),
        "bitbucket" to
            ProviderRequirement(
                missing =
                    "Bitbucket needs API credentials on the server. Add them in Settings → Source Control.",
                unauthenticated =
                    "Bitbucket rejected the configured credentials. Check them in Settings → Source Control.",
            ),
    )

/** `PullRequestUnavailableError.message`. */
fun pullRequestUnavailableMessage(reason: String?, provider: String?): String? {
    val requirement = provider?.let(PROVIDER_REQUIREMENT::get)
    return when (reason) {
        "cli-missing" ->
            requirement?.missing ?: "The tool this host is read through is not installed or set up."
        "cli-unauthenticated" ->
            requirement?.unauthenticated ?: "This host has no working credentials."
        "provider-unsupported" -> "Change requests cannot be browsed for this project's host yet."
        else -> null
    }
}

/** The message a PR read or write failure shows, rebuilt from its tagged error where it has one. */
fun pullRequestErrorMessage(error: Throwable): String {
    val failure = error as? RpcFailure
    val raw = failure?.error
    if (raw != null) {
        when (failure.tag) {
            "PullRequestUnavailableError" ->
                pullRequestUnavailableMessage(raw.string("reason"), raw.string("provider"))
                    ?.let { return it }
            "PullRequestOperationError" -> {
                val detail = raw.string("detail")
                if (detail != null) {
                    return "Pull request operation ${raw.string("operation").orEmpty()} failed: $detail"
                }
            }
        }
    }
    return error.message?.takeIf { it.isNotBlank() } ?: "Couldn't load this pull request."
}

/** `PullRequestOperationError` with `reason: "not-found"`: the PR or its project is gone. */
fun isPullRequestNotFound(error: Throwable): Boolean {
    val failure = error as? RpcFailure ?: return false
    return failure.tag == "PullRequestOperationError" && failure.error?.string("reason") == "not-found"
}

private fun JsonObject.string(key: String): String? =
    (get(key) as? JsonPrimitive)?.takeIf { it.isString }?.content?.takeIf { it.isNotBlank() }
