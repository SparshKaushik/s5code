package club.touchtech.s5code.kotlin.platform

import android.util.Size
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.viewinterop.AndroidView
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.barcode.BarcodeScannerOptions
import com.google.mlkit.vision.common.InputImage
import java.util.concurrent.Executors

/**
 * A live camera preview that reports QR payloads.
 *
 * This is the one place the app uses a View: `PreviewView` owns a `SurfaceView`
 * and there is no Compose equivalent that renders camera frames, so it is hosted
 * in an `AndroidView` and the expressive chrome is drawn around it by the caller.
 *
 * Two details keep it from misbehaving:
 *
 * - **Analysis drops stale frames.** `STRATEGY_KEEP_ONLY_LATEST` means a slow
 *   decode discards frames rather than queuing them, so the preview never lags
 *   behind the camera.
 * - **One result.** [onScanned] is invoked once and the analyzer stops feeding it;
 *   a pairing token consumed twice is a spent credential and a failed pair.
 */
@Composable
fun QrScannerPreview(onScanned: (String) -> Unit, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    // Read through a snapshot so rebinding the camera is never needed just because
    // the caller passed a new lambda instance.
    val callback by rememberUpdatedState(onScanned)
    val previewView = remember { PreviewView(context) }
    val executor = remember { Executors.newSingleThreadExecutor() }

    DisposableEffect(previewView) {
        var delivered = false
        val scanner =
            BarcodeScanning.getClient(
                BarcodeScannerOptions.Builder()
                    .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
                    .build()
            )
        val analysis =
            ImageAnalysis.Builder()
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .setResolutionSelector(
                    ResolutionSelector.Builder()
                        // A pairing URL is a dense QR code; 1280x720 decodes it at
                        // arm's length without asking for a 4K buffer per frame.
                        .setResolutionStrategy(
                            ResolutionStrategy(Size(1280, 720), ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER)
                        )
                        .build()
                )
                .build()
        analysis.setAnalyzer(executor) { proxy ->
            if (delivered) {
                proxy.close()
                return@setAnalyzer
            }
            decodeFrame(scanner, proxy) { value ->
                if (!delivered) {
                    delivered = true
                    callback(value)
                }
            }
        }

        val providerFuture = ProcessCameraProvider.getInstance(context)
        providerFuture.addListener(
            {
                val provider = providerFuture.get()
                val preview =
                    Preview.Builder().build().also { it.surfaceProvider = previewView.surfaceProvider }
                provider.unbindAll()
                runCatching {
                    provider.bindToLifecycle(
                        lifecycleOwner,
                        CameraSelector.DEFAULT_BACK_CAMERA,
                        preview,
                        analysis,
                    )
                }
            },
            androidx.core.content.ContextCompat.getMainExecutor(context),
        )

        onDispose {
            runCatching { providerFuture.get().unbindAll() }
            scanner.close()
            executor.shutdown()
        }
    }

    AndroidView(factory = { previewView }, modifier = modifier)
}

/**
 * Reads one camera frame.
 *
 * The frame is converted with `ImageProxy.toBitmap()` rather than handed to ML Kit
 * as a `MediaImage`. The latter needs CameraX's experimental image accessor and
 * makes the caller responsible for keeping the underlying `Image` alive across an
 * async decode; the conversion costs one bitmap per analyzed frame, which with
 * keep-only-latest backpressure is a handful per second on a screen that exists
 * only to scan.
 */
private fun decodeFrame(
    scanner: com.google.mlkit.vision.barcode.BarcodeScanner,
    proxy: androidx.camera.core.ImageProxy,
    onValue: (String) -> Unit,
) {
    val bitmap = runCatching { proxy.toBitmap() }.getOrNull()
    if (bitmap == null) {
        proxy.close()
        return
    }
    scanner
        .process(InputImage.fromBitmap(bitmap, proxy.imageInfo.rotationDegrees))
        .addOnSuccessListener { barcodes ->
            barcodes.firstNotNullOfOrNull { it.rawValue }?.let(onValue)
        }
        .addOnCompleteListener { proxy.close() }
}
