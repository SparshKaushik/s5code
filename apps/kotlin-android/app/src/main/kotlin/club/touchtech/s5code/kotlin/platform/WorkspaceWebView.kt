package club.touchtech.s5code.kotlin.platform

import android.net.http.SslError
import android.webkit.SslErrorHandler
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView

data class WorkspaceWebNavigation(
    val url: String? = null,
    val canGoBack: Boolean = false,
    val canGoForward: Boolean = false,
)

enum class WorkspaceWebAction { Back, Forward, Reload }

data class WorkspaceWebCommand(val id: Long, val action: WorkspaceWebAction)

private fun WebView.navigation() = WorkspaceWebNavigation(url, canGoBack(), canGoForward())

internal fun sslErrorMessage(primaryError: Int): String =
    when (primaryError) {
        SslError.SSL_DATE_INVALID -> "The preview certificate has an invalid date."
        SslError.SSL_EXPIRED -> "The preview certificate has expired."
        SslError.SSL_IDMISMATCH -> "The preview certificate does not match this server."
        SslError.SSL_NOTYETVALID -> "The preview certificate is not valid yet."
        SslError.SSL_UNTRUSTED -> "The preview certificate is not trusted."
        else -> "The preview's secure connection could not be verified."
    }

/**
 * A `WebView` restricted to one signed workspace asset.
 *
 * This is a deliberate View boundary: HTML and PDF have no Compose renderer.
 * Everything about the configuration is a restriction rather than a feature:
 *
 * - **Same-origin only.** Any navigation to another origin is refused rather than
 *   followed or handed to the browser. A workspace HTML file is untrusted content
 *   — an agent wrote it — and a preview that can navigate is a preview that can
 *   phone home with whatever it read.
 * - **No local access.** File and content-URL access are off, so the page cannot
 *   read the app's own storage through `file://`.
 * - **JavaScript on.** A built report is usually blank without it. That is safe
 *   here precisely because the origin is pinned and local access is off.
 * - **No persistent state.** Caching is disabled and storage is not enabled, so
 *   nothing from a preview survives the screen.
 */
@Composable
// JavaScript is required for built reports. The pinned origin, disabled local
// access/storage, and expiring URL keep that capability inside the preview.
@android.annotation.SuppressLint("SetJavaScriptEnabled")
fun WorkspaceWebView(
    url: String,
    onError: (String) -> Unit,
    modifier: Modifier = Modifier,
    onLoadingChanged: (Boolean) -> Unit = {},
    onNavigationChanged: (WorkspaceWebNavigation) -> Unit = {},
    command: WorkspaceWebCommand? = null,
    onCommandHandled: (Long) -> Unit = {},
) {
    val errorCallback by rememberUpdatedState(onError)
    val loadingCallback by rememberUpdatedState(onLoadingChanged)
    val navigationCallback by rememberUpdatedState(onNavigationChanged)
    val commandHandledCallback by rememberUpdatedState(onCommandHandled)
    val origin = remember(url) { runCatching { java.net.URI(url) }.getOrNull() }

    AndroidView(
        factory = { context ->
            WebView(context).apply {
                settings.javaScriptEnabled = true
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                settings.domStorageEnabled = false
                settings.cacheMode = android.webkit.WebSettings.LOAD_NO_CACHE
                settings.builtInZoomControls = true
                settings.displayZoomControls = false
                settings.useWideViewPort = true
                settings.loadWithOverviewMode = true
                webViewClient =
                    object : WebViewClient() {
                        override fun onPageStarted(
                            view: WebView,
                            url: String?,
                            favicon: android.graphics.Bitmap?,
                        ) {
                            loadingCallback(true)
                            navigationCallback(view.navigation())
                        }

                        override fun onPageFinished(view: WebView, url: String?) {
                            loadingCallback(false)
                            navigationCallback(view.navigation())
                        }

                        override fun shouldOverrideUrlLoading(
                            view: WebView,
                            request: WebResourceRequest,
                        ): Boolean {
                            val target = request.url
                            val sameOrigin =
                                origin != null &&
                                    target.host == origin.host &&
                                    target.scheme == origin.scheme
                            if (!sameOrigin) {
                                errorCallback("This preview cannot open links outside the workspace.")
                            }
                            // True means "handled", which here means "refused".
                            return !sameOrigin
                        }

                        override fun onReceivedSslError(
                            view: WebView,
                            handler: SslErrorHandler,
                            error: SslError,
                        ) {
                            // Workspace HTML is untrusted. Never offer or invoke
                            // proceed(): certificate errors must fail closed.
                            handler.cancel()
                            loadingCallback(false)
                            errorCallback(sslErrorMessage(error.primaryError))
                        }

                        override fun onReceivedError(
                            view: WebView,
                            request: WebResourceRequest,
                            error: WebResourceError,
                        ) {
                            if (request.isForMainFrame) {
                                loadingCallback(false)
                                errorCallback(
                                    error.description?.toString() ?: "The file could not be rendered."
                                )
                            }
                        }
                    }
                loadUrl(url)
            }
        },
        modifier = modifier,
        update = { view ->
            if (view.originalUrl != url) view.loadUrl(url)
            command?.let {
                when (it.action) {
                    WorkspaceWebAction.Back -> if (view.canGoBack()) view.goBack()
                    WorkspaceWebAction.Forward -> if (view.canGoForward()) view.goForward()
                    WorkspaceWebAction.Reload -> view.reload()
                }
                commandHandledCallback(it.id)
            }
        },
        // Destroyed rather than left to the GC: a live WebView keeps a renderer
        // process and its network stack alive after the screen is gone.
        onRelease = WebView::destroy,
    )
}
