package club.touchtech.s5code.kotlin.platform

import android.net.http.SslError
import org.junit.Assert.assertEquals
import org.junit.Test

class WorkspaceWebViewTest {
    @Test
    fun `ssl failures explain the validation problem without suggesting bypass`() {
        assertEquals(
            "The preview certificate has expired.",
            sslErrorMessage(SslError.SSL_EXPIRED),
        )
        assertEquals(
            "The preview certificate is not trusted.",
            sslErrorMessage(SslError.SSL_UNTRUSTED),
        )
        assertEquals(
            "The preview's secure connection could not be verified.",
            sslErrorMessage(999),
        )
    }
}
