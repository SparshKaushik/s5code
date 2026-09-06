package club.touchtech.s5code.kotlin.platform.updates

/**
 * Strict semantic versioning for comparing app releases.
 *
 * Implements SemVer 2.0.0 precedence rules:
 * - Major, minor, and patch are compared numerically.
 * - A normal release has higher precedence than a pre-release version of the same normal version
 *   (e.g., 0.1.0 > 0.1.0-alpha.1).
 * - Pre-release identifiers separated by dots are compared from left to right (numeric identifiers
 *   are compared numerically, non-numeric identifiers are compared lexicographically).
 */
data class SemVer(
    val major: Int,
    val minor: Int,
    val patch: Int,
    val preRelease: String? = null,
    val buildMetadata: String? = null,
) : Comparable<SemVer> {

    override fun compareTo(other: SemVer): Int {
        if (major != other.major) return major.compareTo(other.major)
        if (minor != other.minor) return minor.compareTo(other.minor)
        if (patch != other.patch) return patch.compareTo(other.patch)

        // Pre-release precedence: A version without pre-release is higher than one with pre-release.
        if (preRelease == null && other.preRelease == null) return 0
        if (preRelease == null) return 1
        if (other.preRelease == null) return -1

        val partsA = preRelease.split('.')
        val partsB = other.preRelease.split('.')
        val minLen = minOf(partsA.size, partsB.size)

        for (i in 0 until minLen) {
            val a = partsA[i]
            val b = partsB[i]
            if (a == b) continue

            val aNum = a.toLongOrNull()
            val bNum = b.toLongOrNull()

            return when {
                // Numeric identifiers have lower precedence than non-numeric identifiers.
                aNum != null && bNum != null -> aNum.compareTo(bNum)
                aNum != null -> -1
                bNum != null -> 1
                else -> a.compareTo(b)
            }
        }

        return partsA.size.compareTo(partsB.size)
    }

    override fun toString(): String = buildString {
        append("$major.$minor.$patch")
        if (preRelease != null) append("-$preRelease")
        if (buildMetadata != null) append("+$buildMetadata")
    }

    companion object {
        /**
         * Parses a version string or release tag into a [SemVer].
         *
         * Tolerates common release tag prefixes/suffixes:
         * - "v1.2.3"
         * - "kotlin-android-v0.1.0-alpha.1-debug"
         * - "0.1.0-alpha.1"
         */
        fun parse(raw: String): SemVer? {
            val trimmed = raw.trim()
                .removePrefix("kotlin-android-")
                .removePrefix("v")
                .removePrefix("V")
                .removeSuffix("-debug")

            val withoutBuild = trimmed.substringBefore('+')
            val build = if ('+' in trimmed) trimmed.substringAfter('+').takeIf { it.isNotEmpty() } else null
            val mainParts = withoutBuild.split('-', limit = 2)
            val pre = mainParts.getOrNull(1)?.takeIf { it.isNotEmpty() }
            val numbers = mainParts[0].split('.')
            if (numbers.size != 3) return null
            val major = numbers[0].toIntOrNull() ?: return null
            val minor = numbers[1].toIntOrNull() ?: return null
            val patch = numbers[2].toIntOrNull() ?: return null

            return SemVer(major, minor, patch, pre, build)
        }
    }
}
