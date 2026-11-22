package com.navaq.android.ava

import android.content.Context
import android.os.SystemClock
import android.os.StatFs
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream
import org.apache.commons.compress.compressors.bzip2.BZip2CompressorInputStream
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Installs the multilingual Whisper tiny INT8 model into app-private storage.
 * The one-time model download is the only network operation; recognition itself
 * is performed locally by Sherpa-ONNX after installation.
 */
class AvaSpeechModelManager(context: Context) {
    private val appContext = context.applicationContext
    private val modelDirectory = File(appContext.noBackupFilesDir, "ava/models/whisper-tiny")
    private val archiveFile = File(appContext.cacheDir, "ava-whisper-tiny.tar.bz2")
    private val partialArchiveFile = File(appContext.cacheDir, "ava-whisper-tiny.tar.bz2.part")
    private val archiveValidatorFile = File(appContext.cacheDir, "ava-whisper-tiny.tar.bz2.validator")

    fun isInstalled(): Boolean = REQUIRED_FILES.all { name ->
        File(modelDirectory, name).let { it.isFile && it.length() > MIN_FILE_BYTES[name]!! }
    }

    /** Total size of the installed model files, excluding temporary download data. */
    fun installedSizeBytes(): Long = if (isInstalled()) {
        REQUIRED_FILES.sumOf { File(modelDirectory, it).length() }
    } else {
        0L
    }

    /**
     * Removes only AVA's optional offline speech model.
     * The rest of NAVAQ and AVA's non-speech features are untouched.
     */
    fun deleteModel(): Boolean {
        if (isInstalling.get()) return false
        // Remove both the installed model and any retained partial download so
        // the user's cleanup action actually reclaims all AVA model storage.
        val paths = listOf(
            modelDirectory,
            archiveFile,
            partialArchiveFile,
            archiveValidatorFile
        )
        paths.forEach { file ->
            if (file.exists()) file.deleteRecursively()
        }
        return paths.none { it.exists() }
    }

    fun modelPath(name: String): String {
        require(name in REQUIRED_FILES) { "Unknown AVA model file" }
        return File(modelDirectory, name).absolutePath
    }

    fun install(
        onProgress: (Int) -> Unit,
        onComplete: (Result<Unit>) -> Unit
    ) {
        if (!isInstalling.compareAndSet(false, true)) {
            onComplete(Result.failure(IllegalStateException("AVA model installation is already running")))
            return
        }
        Thread({
            val archive = archiveFile
            val staging = File(modelDirectory.parentFile, "whisper-tiny.installing")
            try {
                val availableBytes = StatFs(appContext.filesDir.absolutePath).availableBytes
                check(availableBytes >= MIN_REQUIRED_FREE_BYTES) {
                    "At least 250 MB of free storage is required to install AVA's offline speech model"
                }
                modelDirectory.parentFile?.mkdirs()
                if (isInstalled()) {
                    onProgress(100)
                    onComplete(Result.success(Unit))
                    return@Thread
                }

                archive.delete()
                staging.deleteRecursively()
                check(staging.mkdirs()) { "Could not create AVA model directory" }

                downloadArchive(archive, onProgress)
                extractModel(archive, staging, onProgress)
                REQUIRED_FILES.forEach { name ->
                    val file = File(staging, name)
                    check(file.isFile && file.length() > MIN_FILE_BYTES[name]!!) {
                        "The downloaded AVA model is incomplete: $name"
                    }
                }

                modelDirectory.deleteRecursively()
                check(staging.renameTo(modelDirectory)) { "Could not activate the AVA speech model" }
                archive.delete()
                onProgress(100)
                onComplete(Result.success(Unit))
            } catch (error: Throwable) {
                archive.delete()
                staging.deleteRecursively()
                onComplete(Result.failure(error))
            } finally {
                isInstalling.set(false)
            }
        }, "ava-model-installer").start()
    }

    /**
     * Downloads to a persistent partial file and resumes with HTTP Range after
     * transient network failures. The archive is promoted only after its full
     * expected byte count has been received.
     */
    private fun downloadArchive(destination: File, onProgress: (Int) -> Unit) {
        if (destination.isFile && destination.length() == EXPECTED_ARCHIVE_BYTES) {
            onProgress(85)
            return
        }

        val partial = File(destination.parentFile, destination.name + ".part")
        val validatorFile = File(destination.parentFile, destination.name + ".validator")
        var existingBytes = partial.length()
        val savedValidator = validatorFile.takeIf { it.isFile }
            ?.readText()
            ?.takeIf { it.isNotBlank() }
        if (existingBytes > EXPECTED_ARCHIVE_BYTES ||
            (existingBytes > 0L && savedValidator == null)
        ) {
            // Resume only when the previous response supplied a validator.
            // Without one, restart instead of risking a mixed-version archive.
            partial.delete()
            validatorFile.delete()
            existingBytes = 0L
        }
        if (existingBytes == EXPECTED_ARCHIVE_BYTES) {
            check(partial.renameTo(destination)) { "Could not finalize the AVA model archive" }
            validatorFile.delete()
            onProgress(85)
            return
        }

        val connection = (URL(MODEL_ARCHIVE_URL).openConnection() as HttpURLConnection).apply {
            connectTimeout = 20_000
            readTimeout = 120_000
            requestMethod = "GET"
            setRequestProperty("User-Agent", "NAVAQ-AVA/1.0")
            setRequestProperty("Accept-Encoding", "identity")
            if (existingBytes > 0L) {
                setRequestProperty("Range", "bytes=$existingBytes-")
                savedValidator?.let { setRequestProperty("If-Range", it) }
            }
        }
        try {
            connection.connect()
            val responseCode = connection.responseCode
            val append: Boolean
            when (responseCode) {
                HttpURLConnection.HTTP_PARTIAL -> {
                    val range = Regex("^bytes (\\d+)-(\\d+)/(\\d+)$")
                        .matchEntire(connection.getHeaderField("Content-Range").orEmpty())
                    val validRange = range != null &&
                        range.groupValues[1].toLong() == existingBytes &&
                        range.groupValues[2].toLong() == EXPECTED_ARCHIVE_BYTES - 1L &&
                        range.groupValues[3].toLong() == EXPECTED_ARCHIVE_BYTES
                    val responseValidator = connection.getHeaderField("ETag")
                        ?: connection.getHeaderField("Last-Modified")
                    if (!validRange || responseValidator.isNullOrBlank() ||
                        responseValidator != savedValidator
                    ) {
                        partial.delete()
                        validatorFile.delete()
                        error("The AVA model changed during download. Retry to start a fresh download.")
                    }
                    append = true
                }
                HttpURLConnection.HTTP_OK -> {
                    // The server ignored Range or the validator changed. Start
                    // a clean transfer rather than joining unrelated versions.
                    partial.delete()
                    validatorFile.delete()
                    existingBytes = 0L
                    val responseLength = connection.getHeaderField("Content-Length")?.toLongOrNull() ?: -1L
                    check(responseLength == EXPECTED_ARCHIVE_BYTES) {
                        "The AVA model archive has an unexpected size"
                    }
                    append = false
                }
                else -> error("AVA model download failed (HTTP $responseCode)")
            }

            val validator = connection.getHeaderField("ETag")
                ?: connection.getHeaderField("Last-Modified")
            if (!validator.isNullOrBlank()) validatorFile.writeText(validator)

            var downloaded = existingBytes
            onProgress(((downloaded * 85L) / EXPECTED_ARCHIVE_BYTES).toInt().coerceIn(0, 85))
            var lastUpdate = 0L
            BufferedInputStream(connection.inputStream).use { input ->
                BufferedOutputStream(FileOutputStream(partial, append)).use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        output.write(buffer, 0, count)
                        downloaded += count
                        check(downloaded <= EXPECTED_ARCHIVE_BYTES) {
                            "The AVA model archive is larger than expected"
                        }
                        val now = SystemClock.elapsedRealtime()
                        if (now - lastUpdate >= 300) {
                            onProgress(((downloaded * 85L) / EXPECTED_ARCHIVE_BYTES).toInt().coerceIn(0, 85))
                            lastUpdate = now
                        }
                    }
                    output.flush()
                }
            }
            check(partial.length() == EXPECTED_ARCHIVE_BYTES) {
                "The AVA model download was interrupted. Retry to resume from the saved data."
            }
            check(partial.renameTo(destination)) { "Could not finalize the AVA model archive" }
            validatorFile.delete()
            onProgress(85)
        } finally {
            connection.disconnect()
        }
    }

    private fun extractModel(archive: File, destination: File, onProgress: (Int) -> Unit) {
        val found = mutableSetOf<String>()
        BZip2CompressorInputStream(BufferedInputStream(FileInputStream(archive))).use { bzip ->
            TarArchiveInputStream(bzip).use { tar ->
                var entry = tar.nextTarEntry
                while (entry != null) {
                    val name = entry.name.substringAfterLast('/')
                    if (!entry.isDirectory && name in REQUIRED_FILES) {
                        val outputFile = File(destination, name)
                        BufferedOutputStream(FileOutputStream(outputFile)).use { output ->
                            val buffer = ByteArray(64 * 1024)
                            var copied = 0L
                            while (true) {
                                val count = tar.read(buffer)
                                if (count < 0) break
                                copied += count
                                check(copied <= MAX_MODEL_FILE_BYTES) { "Invalid AVA model file size" }
                                output.write(buffer, 0, count)
                            }
                            output.flush()
                        }
                        found += name
                    }
                    onProgress(90 + found.size * 3)
                    entry = tar.nextTarEntry
                }
            }
        }
        check(found.containsAll(REQUIRED_FILES)) { "The AVA archive does not contain all required Whisper files" }
    }

    companion object {
        private const val MODEL_ARCHIVE_URL =
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-whisper-tiny.tar.bz2"
        private const val MAX_MODEL_FILE_BYTES = 180L * 1024L * 1024L
        private const val MIN_REQUIRED_FREE_BYTES = 250L * 1024L * 1024L
        private const val EXPECTED_ARCHIVE_BYTES = 116_204_861L
        private val isInstalling = AtomicBoolean(false)
        private val REQUIRED_FILES = setOf(
            "tiny-encoder.int8.onnx",
            "tiny-decoder.int8.onnx",
            "tiny-tokens.txt"
        )
        private val MIN_FILE_BYTES = mapOf(
            "tiny-encoder.int8.onnx" to 1_000_000L,
            "tiny-decoder.int8.onnx" to 1_000_000L,
            "tiny-tokens.txt" to 100_000L
        )
    }
}
