package club.touchtech.s5code.kotlin

import android.animation.ObjectAnimator
import android.app.Activity
import android.os.Build
import android.view.View
import android.view.animation.AccelerateInterpolator
import androidx.annotation.RequiresApi

/**
 * Fades the platform splash out rather than letting it be removed in one frame.
 *
 * The platform API is used directly instead of `androidx.core.splashscreen`: the
 * compat library exists to back-port the splash to older releases, and this app
 * already covers API 26-30 with a window-background splash. Adding a dependency
 * to reach an API the platform hands us on 31+ is not a trade worth making.
 *
 * The listener owns the splash view once it fires, so the animation must call
 * `remove()` when it ends or the splash stays on screen forever.
 */
@RequiresApi(Build.VERSION_CODES.S)
fun Activity.fadeOutSplashScreen() {
    splashScreen.setOnExitAnimationListener { splashView ->
        ObjectAnimator.ofFloat(splashView, View.ALPHA, 1f, 0f).apply {
            duration = SPLASH_FADE_MILLIS
            interpolator = AccelerateInterpolator()
            doOnEnd { splashView.remove() }
            start()
        }
    }
}

/** Long enough to read as a fade, short enough not to delay first input. */
private const val SPLASH_FADE_MILLIS = 180L

private fun ObjectAnimator.doOnEnd(action: () -> Unit) {
    addListener(
        object : android.animation.Animator.AnimatorListener {
            override fun onAnimationStart(animation: android.animation.Animator) = Unit

            override fun onAnimationEnd(animation: android.animation.Animator) = action()

            override fun onAnimationCancel(animation: android.animation.Animator) = action()

            override fun onAnimationRepeat(animation: android.animation.Animator) = Unit
        }
    )
}
