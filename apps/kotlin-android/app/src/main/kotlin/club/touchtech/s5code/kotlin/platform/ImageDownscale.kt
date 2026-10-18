package club.touchtech.s5code.kotlin.platform

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.media.ExifInterface
import club.touchtech.s5code.kotlin.data.downscalableImageMimeType
import club.touchtech.s5code.kotlin.data.fitImageAttachmentSize
import java.io.File

private const val PHOTO_JPEG_QUALITY = 85

/**
 * `fitComposerImage`: shrinks a cached PNG or JPEG whose longest edge is over
 * [club.touchtech.s5code.kotlin.data.IMAGE_ATTACHMENT_MAX_EDGE] in place and
 * returns its new byte size. Null when the image fits, is another format, or
 * anything fails, so the caller keeps the original bytes.
 */
internal fun fitCachedImage(file: File, mimeType: String): Long? {
    val format = downscalableImageMimeType(mimeType) ?: return null
    val temp = File(file.parentFile, "${file.name}.fit")
    return try {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.path, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
        // The re-encode drops EXIF, so the rotation it describes is applied to the pixels.
        val orientation =
            runCatching {
                    ExifInterface(file.path)
                        .getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL)
                }
                .getOrDefault(ExifInterface.ORIENTATION_NORMAL)
        val degrees =
            when (orientation) {
                ExifInterface.ORIENTATION_ROTATE_90 -> 90f
                ExifInterface.ORIENTATION_ROTATE_180 -> 180f
                ExifInterface.ORIENTATION_ROTATE_270 -> 270f
                else -> 0f
            }
        val (width, height) = fitImageAttachmentSize(bounds.outWidth, bounds.outHeight) ?: return null
        var sample = 1
        while (bounds.outWidth / (sample * 2) >= width && bounds.outHeight / (sample * 2) >= height) sample *= 2
        val decoded =
            BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = sample })
                ?: return null
        val scaled =
            if (decoded.width == width && decoded.height == height && degrees == 0f) {
                decoded
            } else {
                val matrix =
                    Matrix().apply {
                        setScale(width.toFloat() / decoded.width, height.toFloat() / decoded.height)
                        if (degrees != 0f) postRotate(degrees)
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
