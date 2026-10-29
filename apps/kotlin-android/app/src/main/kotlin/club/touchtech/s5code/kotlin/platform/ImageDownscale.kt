package club.touchtech.s5code.kotlin.platform

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import androidx.exifinterface.media.ExifInterface
import club.touchtech.s5code.kotlin.data.MAX_BYTE_LIMIT_SHRINKS
import club.touchtech.s5code.kotlin.data.downscalableImageMimeType
import club.touchtech.s5code.kotlin.data.fitImageAttachmentSize
import club.touchtech.s5code.kotlin.data.shrinkImageToByteLimit
import java.io.File

private const val PHOTO_JPEG_QUALITY = 85

/**
 * `fitComposerImage`: shrinks a cached PNG or JPEG whose longest edge is over
 * [club.touchtech.s5code.kotlin.data.IMAGE_ATTACHMENT_MAX_EDGE] in place and
 * returns its new byte size. Null when the image fits, is another format, or
 * anything fails, so the caller keeps the original bytes.
 */
/** Rotate by [degrees], then mirror horizontally when [mirror]: one EXIF orientation applied to pixels. */
internal data class ExifTransform(val degrees: Float, val mirror: Boolean) {
    companion object {
        val Identity = ExifTransform(0f, false)
    }
}

/** All eight EXIF orientations, including the mirrored ones front cameras write. */
internal fun exifOrientationTransform(orientation: Int): ExifTransform =
    when (orientation) {
        ExifInterface.ORIENTATION_FLIP_HORIZONTAL -> ExifTransform(0f, true)
        ExifInterface.ORIENTATION_ROTATE_180 -> ExifTransform(180f, false)
        ExifInterface.ORIENTATION_FLIP_VERTICAL -> ExifTransform(180f, true)
        ExifInterface.ORIENTATION_TRANSPOSE -> ExifTransform(90f, true)
        ExifInterface.ORIENTATION_ROTATE_90 -> ExifTransform(90f, false)
        ExifInterface.ORIENTATION_TRANSVERSE -> ExifTransform(270f, true)
        ExifInterface.ORIENTATION_ROTATE_270 -> ExifTransform(270f, false)
        else -> ExifTransform.Identity
    }

internal fun fitCachedImage(file: File, mimeType: String): Long? {
    val format = downscalableImageMimeType(mimeType) ?: return null
    val temp = File(file.parentFile, "${file.name}.fit")
    return try {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.path, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
        // The re-encode drops EXIF, so the rotation or mirroring it describes is applied to the pixels.
        val orientation =
            runCatching {
                    ExifInterface(file.path)
                        .getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL)
                }
                .getOrDefault(ExifInterface.ORIENTATION_NORMAL)
        val transform = exifOrientationTransform(orientation)
        var (width, height) = fitImageAttachmentSize(bounds.outWidth, bounds.outHeight) ?: return null
        var sample = 1
        while (bounds.outWidth / (sample * 2) >= width && bounds.outHeight / (sample * 2) >= height) sample *= 2
        val decoded =
            BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = sample })
                ?: return null
        try {
            for (shrinks in 0..MAX_BYTE_LIMIT_SHRINKS) {
                val scaled =
                    if (decoded.width == width && decoded.height == height && transform == ExifTransform.Identity) {
                        decoded
                    } else {
                        val matrix =
                            Matrix().apply {
                                setScale(width.toFloat() / decoded.width, height.toFloat() / decoded.height)
                                if (transform.degrees != 0f) postRotate(transform.degrees)
                                if (transform.mirror) postScale(-1f, 1f)
                            }
                        Bitmap.createBitmap(decoded, 0, 0, decoded.width, decoded.height, matrix, true)
                    }
                try {
                    val encoded =
                        temp.outputStream().use { output ->
                            scaled.compress(
                                if (format == "image/png") Bitmap.CompressFormat.PNG else Bitmap.CompressFormat.JPEG,
                                PHOTO_JPEG_QUALITY,
                                output,
                            )
                        }
                    if (!encoded || temp.length() <= 0L) return null
                } finally {
                    if (scaled !== decoded) scaled.recycle()
                }
                if (shrinks == MAX_BYTE_LIMIT_SHRINKS) break
                val smaller = shrinkImageToByteLimit(width, height, temp.length()) ?: break
                width = smaller.first
                height = smaller.second
            }
        } finally {
            decoded.recycle()
        }
        if (!temp.renameTo(file)) {
            temp.copyTo(file, overwrite = true)
        }
        file.length()
    } catch (_: Exception) {
        null
    } catch (_: OutOfMemoryError) {
        null
    } finally {
        temp.delete()
    }
}
