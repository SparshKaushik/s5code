package club.touchtech.s5code.kotlin.platform

import org.junit.Assert.assertEquals
import org.junit.Test

class ImageDownscaleTest {

    @Test
    fun `every exif orientation maps to its rotation and mirror`() {
        // EXIF orientation values 1..8, per the TIFF/EXIF spec.
        val expected =
            mapOf(
                1 to ExifTransform(0f, false),
                2 to ExifTransform(0f, true),
                3 to ExifTransform(180f, false),
                4 to ExifTransform(180f, true),
                5 to ExifTransform(90f, true),
                6 to ExifTransform(90f, false),
                7 to ExifTransform(270f, true),
                8 to ExifTransform(270f, false),
            )
        expected.forEach { (orientation, transform) ->
            assertEquals("orientation $orientation", transform, exifOrientationTransform(orientation))
        }
        assertEquals(ExifTransform.Identity, exifOrientationTransform(0))
    }
}
