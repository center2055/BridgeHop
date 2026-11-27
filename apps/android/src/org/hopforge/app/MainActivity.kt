package org.hopforge.app

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import org.json.JSONArray
import org.json.JSONObject

/**
 * The whole UI is one bundled page (`assets/ui/index.html`); this activity only hosts it and
 * wires it to [EngineService]. Nothing from the network is ever loaded into this WebView.
 */
class MainActivity : Activity() {
    private lateinit var web: WebView
    private val ui = Handler(Looper.getMainLooper())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= 33) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        }
        web = WebView(this)
        with(web.settings) {
            javaScriptEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            setGeolocationEnabled(false)
            mediaPlaybackRequiresUserGesture = true
        }
        web.addJavascriptInterface(Bridge(), "Native")
        web.webViewClient = object : WebViewClient() {
            // The page is bundled with the APK; no other URL may open inside it.
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean =
                request.url.toString() != UI_URL
        }
        setContentView(web)
        EngineService.events = { event ->
            ui.post { if (::web.isInitialized) web.evaluateJavascript("window.onEvent($event);", null) }
        }
        web.loadUrl(UI_URL)
    }

    override fun onDestroy() {
        EngineService.events = null
        web.destroy()
        super.onDestroy()
    }

    /** Methods the page calls through `window.Native`. Every call returns a JSON object. */
    inner class Bridge {
        @JavascriptInterface
        fun state(): String = EngineService.snapshot().toString()

        @JavascriptInterface
        fun connect(): String {
            val req = JSONObject()
                .put("cmd", "connect")
                .put("lines", bundledBridges(this@MainActivity))
                .put("timeout_sec", 150)
            EngineService.enqueue(this@MainActivity, req)
            return JSONObject().put("ok", true).put("started", true).toString()
        }

        @JavascriptInterface
        fun stop(): String = EngineService.command(JSONObject().put("cmd", "stop")).toString()

        @JavascriptInterface
        fun probe(): String {
            val req = JSONObject()
                .put("cmd", "probe")
                .put("lines", bundledBridges(this@MainActivity))
                .put("timeout_ms", 2500)
            return EngineService.command(req).toString()
        }
    }

    companion object {
        const val UI_URL = "file:///android_asset/ui/index.html"

        /** Bridge lines shipped with this build, one per line. */
        fun bundledBridges(context: Context): JSONArray {
            val lines = JSONArray()
            context.assets.open("bridges.txt").bufferedReader().useLines { stream ->
                stream.map { it.trim() }.filter { it.isNotEmpty() && !it.startsWith("#") }
                    .forEach { lines.put(it) }
            }
            return lines
        }
    }
}
