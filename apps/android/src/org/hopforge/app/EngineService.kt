package org.hopforge.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedReader
import java.io.File
import java.io.InputStream
import java.io.InputStreamReader
import java.io.OutputStreamWriter
import java.io.PrintWriter
import java.util.ArrayDeque

/**
 * Foreground service that runs the HopForge engine (`libhopforge.so`, built from
 * `crates/hf-engine`) and relays its JSON-line events to the UI.
 *
 * The engine is a plain executable that reads one JSON command per line on stdin and writes
 * one JSON event per line on stdout. The Android shell and the desktop CLI speak the same
 * protocol, so the engine is tested on a desktop with the exact same code path.
 */
class EngineService : Service() {
    private var proc: Process? = null
    private var toEngine: PrintWriter? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
        startInForeground()
        startEngine()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopEngine()
            stopSelf()
            return START_NOT_STICKY
        }
        return START_STICKY
    }

    override fun onDestroy() {
        stopEngine()
        instance = null
        super.onDestroy()
    }

    private fun startInForeground() {
        val manager = getSystemService(NotificationManager::class.java)
        val launch = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, EngineService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE
        )
        val builder = if (Build.VERSION.SDK_INT >= 26) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL, string("notif_channel"), NotificationManager.IMPORTANCE_LOW)
            )
            Notification.Builder(this, CHANNEL)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        val notification = builder
            .setSmallIcon(android.R.drawable.ic_secure)
            .setContentTitle(string("notif_title"))
            .setContentText(string("notif_text"))
            .setContentIntent(launch)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .addAction(0, string("notif_stop"), stop)
            .build()
        if (Build.VERSION.SDK_INT >= 34) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun string(name: String): String {
        val id = resources.getIdentifier(name, "string", packageName)
        return if (id != 0) getString(id) else name
    }

    private fun startEngine() {
        if (proc != null) return
        val engine = File(applicationInfo.nativeLibraryDir, ENGINE_BIN)
        val child = ProcessBuilder(engine.path, "serve")
            .apply { environment()["HOME"] = filesDir.absolutePath }
            .start()
        proc = child
        toEngine = PrintWriter(OutputStreamWriter(child.outputStream, Charsets.UTF_8), true)
        Thread {
            drain(child.inputStream) { line -> handleLine(line) }
            onEngineExit()
        }.apply { isDaemon = true }.start()
        Thread {
            drain(child.errorStream) { line -> record(JSONObject().put("event", "log").put("detail", line)) }
        }.apply { isDaemon = true }.start()
        flushPending()
    }

    private fun drain(stream: InputStream, sink: (String) -> Unit) {
        try {
            BufferedReader(InputStreamReader(stream, Charsets.UTF_8)).useLines { lines ->
                lines.forEach { if (it.isNotBlank()) sink(it) }
            }
        } catch (_: Exception) {
            // The pipe closing is the normal end of a session.
        }
    }

    private fun handleLine(line: String) {
        val event = try {
            JSONObject(line)
        } catch (_: Exception) {
            return
        }
        when (event.optString("event")) {
            "progress" -> {
                phase = "connecting"
                progress = event.optInt("progress", progress)
                detail = event.optString("summary", detail)
            }
            "tier" -> if (event.optBoolean("pass")) {
                when (event.optString("tier")) {
                    "T4" -> phase = "bootstrapped"
                    "T5" -> phase = "connected"
                }
            }
            "connected" -> {
                phase = "connected"
                progress = 100
                socksPort = event.optInt("socks_port", socksPort)
                httpPort = event.optInt("http_port", httpPort)
                detail = "connected"
            }
            "failed" -> {
                phase = "failed"
                detail = event.optString("reason", detail)
            }
        }
        record(event)
    }

    private fun record(event: JSONObject) {
        synchronized(history) {
            history.addLast(event.toString())
            while (history.size > MAX_HISTORY) history.removeFirst()
        }
        events?.invoke(event)
    }

    private fun onEngineExit() {
        synchronized(this) {
            proc = null
            toEngine = null
        }
        if (phase != "idle") {
            phase = "failed"
            detail = "engine-exited"
            record(JSONObject().put("event", "failed").put("reason", "engine-exited"))
        }
    }

    private fun stopEngine() {
        synchronized(this) {
            toEngine?.close()
            proc?.let { child ->
                child.destroy()
                val deadline = System.currentTimeMillis() + 3_000
                while (child.isAlive && System.currentTimeMillis() < deadline) {
                    try {
                        Thread.sleep(50)
                    } catch (_: InterruptedException) {
                        Thread.currentThread().interrupt()
                    }
                }
                if (child.isAlive) child.destroyForcibly()
            }
            proc = null
            toEngine = null
        }
        phase = "idle"
        progress = 0
        socksPort = 0
        httpPort = 0
    }

    private fun send(line: String): Boolean {
        synchronized(this) {
            val writer = toEngine ?: return false
            return try {
                writer.println(line)
                true
            } catch (_: Exception) {
                false
            }
        }
    }

    companion object {
        private const val CHANNEL = "hopforge"
        private const val NOTIFICATION_ID = 1
        private const val ENGINE_BIN = "libhopforge.so"
        private const val MAX_HISTORY = 400

        @Volatile var phase = "idle"
        @Volatile var progress = 0
        @Volatile var detail = ""
        @Volatile var socksPort = 0
        @Volatile var httpPort = 0
        @Volatile var events: ((JSONObject) -> Unit)? = null
        val history = ArrayDeque<String>()
        private val pending = ArrayDeque<String>()
        @Volatile private var instance: EngineService? = null

        /** Starts the service if needed, then makes sure the command reaches the engine. */
        fun enqueue(context: Context, request: JSONObject) {
            synchronized(pending) { pending.addLast(request.toString()) }
            context.startForegroundService(Intent(context, EngineService::class.java))
            instance?.flushPending()
        }

        /** Sends a command to a running engine; `ok=false` when no engine is running. */
        fun command(request: JSONObject): JSONObject {
            val sent = instance?.send(request.toString()) ?: false
            return JSONObject().put("ok", sent)
        }

        private fun flushPending() {
            while (true) {
                val line = synchronized(pending) { pending.pollFirst() } ?: return
                send(line)
            }
        }

        fun snapshot(): JSONObject {
            val copy = JSONArray()
            synchronized(history) { history.forEach { copy.put(it) } }
            return JSONObject()
                .put("phase", phase)
                .put("progress", progress)
                .put("detail", detail)
                .put("socks_port", socksPort)
                .put("http_port", httpPort)
                .put("history", copy)
        }
    }
}
