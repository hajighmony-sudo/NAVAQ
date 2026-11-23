package com.navaq.android.net

import android.content.Context
import android.net.Uri
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.SystemClock
import org.json.JSONException
import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.IOException
import java.io.InputStream
import java.net.Inet4Address
import java.net.InetSocketAddress
import java.net.NetworkInterface
import java.net.ServerSocket
import java.net.Socket
import java.security.SecureRandom
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors

/**
 * Serverless "listen together" on the local Wi-Fi, like pairing a phone with a TV.
 * The host publishes the current track over a tiny TCP server (JSON lines for state and
 * commands, plain HTTP with Range for the audio itself); guests find it through mDNS or an
 * address, enter a pairing code and follow the host's track and position.
 */
object LanRoom {
    const val SERVICE_TYPE = "_navaq._tcp."
    @Volatile var host: Host? = null
    @Volatile var guest: Guest? = null

    // Set by whichever activity instance is alive, so rotation does not break an open room.
    @Volatile var listener: ((String) -> Unit)? = null
    @Volatile var commandListener: ((JSONObject) -> Unit)? = null
    @Volatile var resolver: ((String) -> Uri?)? = null

    fun localIp(): String? {
        try {
            for (ni in NetworkInterface.getNetworkInterfaces()?.toList().orEmpty()) {
                if (!ni.isUp || ni.isLoopback) continue
                for (a in ni.inetAddresses.toList()) {
                    if (a is Inet4Address && a.isSiteLocalAddress) return a.hostAddress
                }
            }
        } catch (_: Exception) {
        }
        return null
    }

    fun newCode(): String = "%06d".format(SecureRandom().nextInt(1_000_000))
    fun newToken(): String = (1..16).map { "abcdefghijklmnopqrstuvwxyz0123456789"[SecureRandom().nextInt(36)] }.joinToString("")

    internal fun readLine(input: InputStream): String? {
        val sb = StringBuilder()
        while (sb.length < 8192) {
            val b = input.read()
            if (b < 0) return if (sb.isEmpty()) null else sb.toString()
            if (b == '\n'.code) return sb.toString().trimEnd('\r')
            sb.append(b.toChar())
        }
        return null
    }

    private class Client(val sock: Socket) {
        private val out = sock.getOutputStream()
        @Synchronized
        fun send(line: String): Boolean = try {
            out.write(line.toByteArray(Charsets.UTF_8)); out.flush(); true
        } catch (_: IOException) {
            false
        }
    }

    class Host(
        private val ctx: Context,
        val code: String,
        private val token: String,
        private val resolve: (String) -> Uri?,
        private val onCommand: (JSONObject) -> Unit
    ) {
        private val server = ServerSocket(0)
        val port: Int = server.localPort
        private val clients = CopyOnWriteArrayList<Client>()
        private val pool = Executors.newCachedThreadPool()
        @Volatile private var state = "{\"t\":\"state\"}"
        @Volatile private var running = true
        @Volatile private var failedHellos = 0
        @Volatile var allowGuestControl = true
        val guestCount: Int get() = clients.size
        private var nsd: NsdManager? = null
        private var registration: NsdManager.RegistrationListener? = null

        fun start(deviceName: String) {
            pool.execute { acceptLoop() }
            try {
                val manager = ctx.getSystemService(Context.NSD_SERVICE) as NsdManager
                val info = NsdServiceInfo().apply {
                    serviceName = "NAVAQ-$deviceName"
                    serviceType = SERVICE_TYPE
                    port = this@Host.port
                }
                val listener = object : NsdManager.RegistrationListener {
                    override fun onServiceRegistered(i: NsdServiceInfo) = Unit
                    override fun onRegistrationFailed(i: NsdServiceInfo, code: Int) = Unit
                    override fun onServiceUnregistered(i: NsdServiceInfo) = Unit
                    override fun onUnregistrationFailed(i: NsdServiceInfo, code: Int) = Unit
                }
                manager.registerService(info, NsdManager.PROTOCOL_DNS_SD, listener)
                nsd = manager
                registration = listener
            } catch (_: Exception) {
                // Discovery is optional: guests can still connect by address.
            }
        }

        fun update(json: JSONObject) {
            state = json.put("t", "state").toString()
            val line = state + "\n"
            pool.execute { clients.forEach { if (!it.send(line)) clients.remove(it) } }
        }

        fun stop() {
            running = false
            try { registration?.let { nsd?.unregisterService(it) } } catch (_: Exception) { }
            try { server.close() } catch (_: IOException) { }
            clients.forEach { try { it.sock.close() } catch (_: IOException) { } }
            clients.clear()
            pool.shutdownNow()
        }

        private fun acceptLoop() {
            while (running) {
                val sock = try { server.accept() } catch (_: IOException) { return }
                pool.execute { handle(sock) }
            }
        }

        private fun handle(sock: Socket) {
            try {
                val input = BufferedInputStream(sock.getInputStream())
                val first = readLine(input) ?: return
                if (first.startsWith("GET ")) serve(sock, input, first) else control(sock, input, first)
            } catch (_: Exception) {
            } finally {
                try { sock.close() } catch (_: IOException) { }
            }
        }

        private fun control(sock: Socket, input: InputStream, first: String) {
            val hello = try { JSONObject(first) } catch (_: JSONException) { return }
            if (failedHellos >= 10 || hello.optString("code") != code) {
                failedHellos++
                sock.getOutputStream().write("{\"t\":\"err\",\"msg\":\"bad_code\"}\n".toByteArray())
                return
            }
            val client = Client(sock)
            clients.add(client)
            client.send("{\"t\":\"hello\",\"token\":\"$token\"}\n")
            client.send(state + "\n")
            try {
                while (running) {
                    val line = readLine(input) ?: break
                    val cmd = try { JSONObject(line) } catch (_: JSONException) { continue }
                    when (cmd.optString("t")) {
                        "ping" -> client.send(
                            JSONObject().put("t", "pong").put("c", cmd.optLong("c"))
                                .put("h", SystemClock.elapsedRealtime()).toString() + "\n"
                        )
                        "cmd" -> if (allowGuestControl) onCommand(cmd)
                    }
                }
            } finally {
                clients.remove(client)
            }
        }

        private fun reply(sock: Socket, status: String) {
            sock.getOutputStream().write("HTTP/1.1 $status\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".toByteArray())
        }

        private fun serve(sock: Socket, input: InputStream, requestLine: String) {
            var range: String? = null
            while (true) {
                val line = readLine(input) ?: return
                if (line.isEmpty()) break
                if (line.startsWith("Range:", true)) range = line.substring(6).trim()
            }
            val target = requestLine.split(" ").getOrNull(1) ?: return
            val uri = Uri.parse("http://x$target")
            val segments = uri.pathSegments
            if (uri.getQueryParameter("k") != token || segments.size != 2 || segments[0] != "a") return reply(sock, "403 Forbidden")
            val source = resolve(segments[1]) ?: return reply(sock, "404 Not Found")
            val afd = try { ctx.contentResolver.openAssetFileDescriptor(source, "r") } catch (_: Exception) { null }
                ?: return reply(sock, "404 Not Found")
            afd.use {
                val total = it.length
                var start = 0L
                var end = if (total > 0) total - 1 else -1L
                var partial = false
                if (total > 0 && range != null) {
                    val m = Regex("bytes=(\\d*)-(\\d*)").matchEntire(range!!)
                    if (m != null) {
                        val a = m.groupValues[1]
                        val b = m.groupValues[2]
                        if (a.isNotEmpty()) {
                            start = a.toLong()
                            if (b.isNotEmpty()) end = minOf(b.toLong(), total - 1)
                        } else if (b.isNotEmpty()) {
                            start = maxOf(0L, total - b.toLong())
                        }
                        if (start > end) return reply(sock, "416 Range Not Satisfiable")
                        partial = true
                    }
                }
                val out = sock.getOutputStream()
                val head = StringBuilder(if (partial) "HTTP/1.1 206 Partial Content\r\n" else "HTTP/1.1 200 OK\r\n")
                head.append("Content-Type: audio/mpeg\r\nAccept-Ranges: bytes\r\nConnection: close\r\n")
                if (total > 0) {
                    head.append("Content-Length: ${end - start + 1}\r\n")
                    if (partial) head.append("Content-Range: bytes $start-$end/$total\r\n")
                }
                out.write(head.append("\r\n").toString().toByteArray())
                val stream = it.createInputStream()
                var skipped = 0L
                while (skipped < start) {
                    val s = stream.skip(start - skipped)
                    if (s <= 0) break
                    skipped += s
                }
                var left = if (total > 0) end - start + 1 else Long.MAX_VALUE
                val buf = ByteArray(32 * 1024)
                while (left > 0) {
                    val n = stream.read(buf, 0, minOf(buf.size.toLong(), left).toInt())
                    if (n < 0) break
                    out.write(buf, 0, n)
                    left -= n
                }
                out.flush()
            }
        }
    }

    class Guest(
        val host: String,
        val port: Int,
        private val code: String,
        private val onEvent: (String) -> Unit
    ) {
        @Volatile var token = ""
        @Volatile var lastState: JSONObject? = null
        @Volatile var lastAt = 0L
        @Volatile var connected = false

        /** Estimated (host clock - guest clock) in ms, from the lowest-latency ping exchange. */
        @Volatile var offsetMs = 0L
        @Volatile var rttMs = 0L
        private val samples = ArrayList<LongArray>()
        private var sock: Socket? = null
        @Volatile private var open = true

        fun start() {
            Thread {
                try {
                    val s = Socket()
                    s.connect(InetSocketAddress(host, port), 6000)
                    sock = s
                    val out = s.getOutputStream()
                    out.write((JSONObject().put("t", "hello").put("code", code).toString() + "\n").toByteArray())
                    out.flush()
                    val input = BufferedInputStream(s.getInputStream())
                    while (open) {
                        val line = readLine(input) ?: break
                        val o = try { JSONObject(line) } catch (_: JSONException) { continue }
                        when (o.optString("t")) {
                            "hello" -> { token = o.optString("token"); connected = true; startPinger(); onEvent("connected") }
                            "pong" -> {
                                val now = SystemClock.elapsedRealtime()
                                val rtt = now - o.optLong("c")
                                val off = o.optLong("h") + rtt / 2 - now
                                synchronized(samples) {
                                    samples += longArrayOf(rtt, off)
                                    if (samples.size > 8) samples.removeAt(0)
                                    val best = samples.minByOrNull { it[0] }!!
                                    rttMs = best[0]; offsetMs = best[1]
                                }
                            }
                            "state" -> { lastState = o; lastAt = SystemClock.elapsedRealtime(); onEvent("state") }
                            "err" -> { onEvent("bad_code"); break }
                        }
                    }
                } catch (_: Exception) {
                } finally {
                    val was = connected
                    connected = false
                    if (open) onEvent(if (was) "closed" else "failed")
                }
            }.start()
        }

        private fun startPinger() {
            Thread {
                var n = 0
                while (open && connected) {
                    val s = sock ?: break
                    try {
                        s.getOutputStream().write((JSONObject().put("t", "ping").put("c", SystemClock.elapsedRealtime()).toString() + "\n").toByteArray())
                        s.getOutputStream().flush()
                    } catch (_: IOException) {
                        break
                    }
                    n++
                    try { Thread.sleep(if (n < 6) 200L else 2000L) } catch (_: InterruptedException) { break }
                }
            }.start()
        }

        fun send(cmd: JSONObject) {
            val s = sock ?: return
            Thread {
                try {
                    s.getOutputStream().write((cmd.put("t", "cmd").toString() + "\n").toByteArray())
                    s.getOutputStream().flush()
                } catch (_: IOException) {
                }
            }.start()
        }

        fun urlFor(path: String) = "http://$host:$port$path?k=$token"

        fun leave() {
            open = false
            try { sock?.close() } catch (_: IOException) { }
        }
    }

    class Found(val name: String, val host: String, val port: Int)

    class Discovery(private val ctx: Context, private val onChange: (List<Found>) -> Unit) {
        private val manager = ctx.getSystemService(Context.NSD_SERVICE) as NsdManager
        private val found = LinkedHashMap<String, Found>()
        private val pending = ArrayDeque<NsdServiceInfo>()
        private var resolving = false
        private var listener: NsdManager.DiscoveryListener? = null

        fun start() {
            val l = object : NsdManager.DiscoveryListener {
                override fun onDiscoveryStarted(t: String) = Unit
                override fun onStartDiscoveryFailed(t: String, e: Int) = Unit
                override fun onStopDiscoveryFailed(t: String, e: Int) = Unit
                override fun onDiscoveryStopped(t: String) = Unit
                override fun onServiceFound(info: NsdServiceInfo) {
                    synchronized(this@Discovery) { pending.addLast(info) }
                    next()
                }
                override fun onServiceLost(info: NsdServiceInfo) {
                    synchronized(this@Discovery) { found.remove(info.serviceName) }
                    publish()
                }
            }
            try {
                manager.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, l)
                listener = l
            } catch (_: Exception) {
            }
        }

        private fun publish() = onChange(synchronized(this) { found.values.toList() })

        private fun next() {
            val info = synchronized(this) {
                if (resolving) return
                val i = pending.removeFirstOrNull() ?: return
                resolving = true
                i
            }
            try {
                @Suppress("DEPRECATION")
                manager.resolveService(info, object : NsdManager.ResolveListener {
                    override fun onResolveFailed(i: NsdServiceInfo, e: Int) = done()
                    override fun onServiceResolved(r: NsdServiceInfo) {
                        val address = r.host?.hostAddress
                        if (address != null) synchronized(this@Discovery) { found[r.serviceName] = Found(r.serviceName.removePrefix("NAVAQ-"), address, r.port) }
                        publish()
                        done()
                    }
                })
            } catch (_: Exception) {
                done()
            }
        }

        private fun done() {
            synchronized(this) { resolving = false }
            next()
        }

        fun stop() {
            try { listener?.let { manager.stopServiceDiscovery(it) } } catch (_: Exception) { }
            listener = null
        }
    }
}
