package org.hopforge.app

import android.Manifest
import android.app.Activity
import android.content.Context
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.View
import android.view.WindowInsets
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayInputStream

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
            // The page is bundled with the APK: nothing else may be read, opened or loaded here.
            allowFileAccess = false
            allowContentAccess = false
            if (Build.VERSION.SDK_INT < 30) blockFileCrossOrigin()
            setGeolocationEnabled(false)
            mediaPlaybackRequiresUserGesture = true
            javaScriptCanOpenWindowsAutomatically = false
            setSupportMultipleWindows(false)
            mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
            setDomStorageEnabled(false)
            if (Build.VERSION.SDK_INT >= 26) setSafeBrowsingEnabled(true)
        }
        web.addJavascriptInterface(Bridge(), "Native")
        web.webViewClient = object : WebViewClient() {
            // The page is bundled with the APK; no other URL may open inside it.
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean =
                request.url.toString() != UI_URL

            // ...and no other URL may even be fetched from inside it: every request that is not
            // for a bundled asset is answered with an empty response.
            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
                if (request.url.toString().startsWith(ASSET_PREFIX)) null else EMPTY_RESPONSE
        }
        setContentView(web)
        // Apps targeting SDK 35 and later draw behind the system bars on Android 15 and later
        // (developer.android.com/develop/ui/views/layout/edge-to-edge), so the page is moved
        // clear of the bars and of any display cutout. Older systems already fit the window.
        web.setOnApplyWindowInsetsListener { view, insets -> padForSystemBars(view, insets) }
        web.requestApplyInsets()
        EngineService.events = { event ->
            ui.post { if (::web.isInitialized) web.evaluateJavascript("window.onEvent(${js(event)});", null) }
        }
        web.loadUrl(UI_URL)
    }

    /** Keeps the page out from under the status bar, the navigation bar and the cutout. */
    private fun padForSystemBars(view: View, insets: WindowInsets): WindowInsets {
        if (Build.VERSION.SDK_INT >= 30) {
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
        } else {
            @Suppress("DEPRECATION")
            view.setPadding(
                insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
                insets.systemWindowInsetRight, insets.systemWindowInsetBottom
            )
        }
        return insets
    }

    /** The event as a JavaScript string literal, parsed by the page: no event text can become code. */
    private fun js(event: JSONObject): String = JSONObject.quote(event.toString())

    override fun onDestroy() {
        EngineService.events = null
        web.destroy()
        super.onDestroy()
    }

    /** Ignored from API 30 on, where both are always false; explicit for older releases. */
    @Suppress("DEPRECATION")
    private fun WebSettings.blockFileCrossOrigin() {
        allowFileAccessFromFileURLs = false
        allowUniversalAccessFromFileURLs = false
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
        private const val ASSET_PREFIX = "file:///android_asset/"

        /** What every request for something that is not bundled with the app receives: nothing. */
        private val EMPTY_RESPONSE =
            WebResourceResponse("text/plain", "utf-8", ByteArrayInputStream(ByteArray(0)))

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
