package sh.cydonia.remote

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.net.ConnectivityManager
import android.net.Network
import android.net.Uri
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.text.method.PasswordTransformationMethod
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher

class MainActivity : Activity() {
  private var web: WebView? = null
  private var pages: Pages? = null
  private val watchdog = Handler(Looper.getMainLooper())
  private var chooser: ValueCallback<Array<Uri>>? = null
  private val backCallback by lazy { OnBackInvokedCallback { goBack() } }
  private val networkCallback = object : ConnectivityManager.NetworkCallback() {
    override fun onAvailable(network: Network) {
      runOnUiThread { nudge() }
    }
  }

  override fun onCreate(state: Bundle?) {
    super.onCreate(state)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      onBackInvokedDispatcher.registerOnBackInvokedCallback(
        OnBackInvokedDispatcher.PRIORITY_DEFAULT,
        backCallback,
      )
    }
    getSystemService(ConnectivityManager::class.java)?.registerDefaultNetworkCallback(networkCallback)
    askToNotify()
    val offered = Connection.from(intent?.data)
    if (offered != null) askFor(offered) else open(Connection.load(this))
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    Connection.from(intent.data)?.let { askFor(it) }
  }

  override fun onResume() {
    super.onResume()
    visible = true
    watchAgents()
    nudge()
  }

  private fun askToNotify() {
    val needed = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
      checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
    if (needed) requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), NOTIFY)
  }

  private fun watchAgents() {
    if (Connection.load(this).complete) NoticeService.start(this)
  }

  override fun onPause() {
    visible = false
    web?.evaluateJavascript("window.dispatchEvent(new Event('cydonia-pause'))", null)
    super.onPause()
  }

  override fun onDestroy() {
    runCatching { getSystemService(ConnectivityManager::class.java)?.unregisterNetworkCallback(networkCallback) }
    super.onDestroy()
  }

  private fun nudge() {
    web?.evaluateJavascript("window.dispatchEvent(new Event('cydonia-resume'))", null)
  }

  @Deprecated("Deprecated in Java")
  override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
    super.onActivityResult(requestCode, resultCode, data)
    if (requestCode != PICK_FILES) return
    chooser?.onReceiveValue(if (resultCode == RESULT_OK) chosen(data) else null)
    chooser = null
  }

  private fun chosen(data: Intent?): Array<Uri>? {
    val clip = data?.clipData
    if (clip != null) return Array(clip.itemCount) { clip.getItemAt(it).uri }
    return data?.data?.let { arrayOf(it) }
  }

  private fun choose(callback: ValueCallback<Array<Uri>>, params: WebChromeClient.FileChooserParams): Boolean {
    chooser?.onReceiveValue(null)
    chooser = callback
    val intent = params.createIntent().apply {
      if (params.mode == WebChromeClient.FileChooserParams.MODE_OPEN_MULTIPLE) {
        putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
      }
    }
    return runCatching { startActivityForResult(intent, PICK_FILES) }
      .onFailure { chooser = null }
      .isSuccess
  }

  @Deprecated("Deprecated in Java")
  override fun onBackPressed() {
    goBack()
  }

  private fun goBack() {
    if (pages?.back() == true) return
    val view = web ?: return finish()
    view.evaluateJavascript("window.cydoniaBack ? window.cydoniaBack() : false") { taken ->
      if (taken != "true") moveTaskToBack(true)
    }
  }

  fun switchTo(connection: Connection) {
    connection.save(this)
    Hosts.remember(this, connection)
    watchAgents()
    open(connection)
  }

  private fun open(connection: Connection) {
    when {
      !connection.complete -> askFor(connection)
      !connection.private -> askFor(connection, getString(R.string.not_private, connection.host))
      else -> load(connection)
    }
  }

  private fun column(): LinearLayout =
    LinearLayout(this).apply {
      orientation = LinearLayout.VERTICAL
      gravity = Gravity.CENTER
      val gutter = (24 * resources.displayMetrics.density).toInt()
      setPadding(gutter, gutter, gutter, gutter)
    }

  private fun label(text: String): TextView =
    TextView(this).apply {
      this.text = text
      textSize = 18f
      gravity = Gravity.CENTER
      setPadding(0, 0, 0, (16 * resources.displayMetrics.density).toInt())
    }

  private fun button(text: String, action: () -> Unit): Button =
    Button(this).apply {
      this.text = text
      setOnClickListener { action() }
    }

  private fun show(view: View) {
    watchdog.removeCallbacksAndMessages(null)
    pages?.clear()
    pages = null
    web?.destroy()
    web = null
    setContentView(view)
  }

  private fun askFor(connection: Connection, problem: String? = null) {
    val address = EditText(this).apply {
      hint = getString(R.string.address_hint)
      setText(connection.address)
      inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
      isSingleLine = true
    }
    val token = EditText(this).apply {
      hint = getString(R.string.token_hint)
      setText(connection.token)
      inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
      isSingleLine = true
      transformationMethod = PasswordTransformationMethod.getInstance()
    }
    show(column().apply {
      addView(label(getString(R.string.connect_title)))
      problem?.let { addView(label(it)) }
      addView(address, LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT))
      addView(token, LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT))
      addView(button(getString(R.string.connect)) {
        val entered = Connection(address.text.toString().trim(), token.text.toString().trim())
        switchTo(entered)
      })
    })
  }

  private fun offline(connection: Connection) {
    show(column().apply {
      addView(label(getString(R.string.offline, connection.address)))
      addView(button(getString(R.string.retry)) { load(connection) })
      addView(button(getString(R.string.change)) { askFor(connection) })
      Hosts.saved(this@MainActivity)
        .filter { it.address != connection.address }
        .forEach { other -> addView(button(getString(R.string.use_host, other.address)) { switchTo(other) }) }
    })
  }

  private fun load(connection: Connection) {
    val view = WebView(this).apply {
      setBackgroundColor(Color.BLACK)
      setLayerType(View.LAYER_TYPE_HARDWARE, null)
      setRendererPriorityPolicy(WebView.RENDERER_PRIORITY_IMPORTANT, true)
      isLongClickable = false
      isHapticFeedbackEnabled = false
      setOnLongClickListener { true }
      settings.offscreenPreRaster = true
      settings.javaScriptEnabled = true
      settings.domStorageEnabled = true
      settings.allowFileAccess = false
      settings.allowContentAccess = false
      webChromeClient = object : WebChromeClient() {
        override fun onShowFileChooser(
          view: WebView,
          callback: ValueCallback<Array<Uri>>,
          params: FileChooserParams,
        ): Boolean = choose(callback, params)
      }
      webViewClient = object : WebViewClient() {
        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
          if (request.url.host == connection.host) return false
          runCatching { startActivity(Intent(Intent.ACTION_VIEW, request.url)) }
          return true
        }

        override fun onPageCommitVisible(view: WebView, url: String) {
          watchdog.removeCallbacksAndMessages(null)
        }

        override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
          if (request.isForMainFrame) offline(connection)
        }

        override fun onReceivedHttpError(view: WebView, request: WebResourceRequest, response: WebResourceResponse) {
          if (request.isForMainFrame) offline(connection)
        }
      }
    }
    val layer = FrameLayout(this)
    val stack = FrameLayout(this).apply {
      addView(view, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
      addView(layer, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
    }
    show(stack)
    val shown = Pages(this, layer)
    view.addJavascriptInterface(shown, "CydoniaShell")
    view.addJavascriptInterface(Hosts(this, connection), "CydoniaHosts")
    pages = shown
    view.isFocusableInTouchMode = true
    view.requestFocus()
    web = view
    watchdog.postDelayed({ offline(connection) }, LOAD_TIMEOUT)
    view.loadUrl(connection.page)
  }

  companion object {
    private const val LOAD_TIMEOUT = 8000L
    private const val PICK_FILES = 7
    private const val NOTIFY = 8

    @Volatile var visible = false
  }
}
