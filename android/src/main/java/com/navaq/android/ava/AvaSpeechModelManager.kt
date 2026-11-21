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

    fun isInstalled(): Boolean = REQUIRED_FILES.all { name ->
        File(modelDirectory, name).let { it.isFile && it.length() > MIN_FILE_BYTES[name]!! }
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
            val archive = File(appContext.cacheDir, "ava-whisper-tiny.tar.bz2")
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

    private fun downloadArchive(destination: File, onProgress: (Int) -> Unit) {
        val connection = (URL(MODEL_ARCHIVE_URL).openConnection() as HttpURLConnection).apply {
            connectTimeout = 20_000
            readTimeout = 120_000
            requestMethod = "GET"
            setRequestProperty("User-Agent", "NAVAQ-AVA/1.0")
            setRequestProperty("Accept-Encoding", "identity")
        }
        try {
            connection.connect()
            check(connection.responseCode == HttpURLConnection.HTTP_OK) {
                "AVA model download failed (HTTP ${connection.responseCode})"
            }
            val total = connection.contentLength.toLong()
            var downloaded = 0L
            var lastUpdate = 0L
            BufferedInputStream(connection.inputStream).use { input ->
                BufferedOutputStream(FileOutputStream(destination)).use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        output.write(buffer, 0, count)
                        downloaded += count
                        val now = SystemClock.elapsedRealtime()
                        if (now - lastUpdate >= 300) {
                            val percent = if (total > 0) ((downloaded * 85) / total).toInt() else 1
                            onProgress(percent.coerceIn(1, 85))
                            lastUpdate = now
                        }
                    }
                    output.flush()
                }
            }
            check(destination.length() == EXPECTED_ARCHIVE_BYTES) {
                "The AVA speech model archive is incomplete or has an unexpected size"
            }
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
