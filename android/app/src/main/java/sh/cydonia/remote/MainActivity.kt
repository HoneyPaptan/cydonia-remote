package sh.cydonia.remote

import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.text.method.PasswordTransformationMethod
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView

class MainActivity : Activity() {
  private var web: WebView? = null
  private val watchdog = Handler(Looper.getMainLooper())

  override fun onCreate(state: Bundle?) {
    super.onCreate(state)
    val offered = Connection.from(intent?.data)
    if (offered != null) askFor(offered) else open(Connection.load(this))
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    Connection.from(intent.data)?.let { askFor(it) }
  }

  override fun onResume() {
    super.onResume()
    web?.evaluateJavascript("window.dispatchEvent(new Event('cydonia-resume'))", null)
  }

  @Deprecated("Deprecated in Java")
  override fun onBackPressed() {
    if (web != null) moveTaskToBack(true) else super.onBackPressed()
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
        entered.save(this@MainActivity)
        open(entered)
      })
    })
  }

  private fun offline(connection: Connection) {
    show(column().apply {
      addView(label(getString(R.string.offline, connection.address)))
      addView(button(getString(R.string.retry)) { load(connection) })
      addView(button(getString(R.string.change)) { askFor(connection) })
    })
  }

  private fun load(connection: Connection) {
    val view = WebView(this).apply {
      setBackgroundColor(Color.BLACK)
      settings.javaScriptEnabled = true
      settings.domStorageEnabled = true
      settings.allowFileAccess = false
      settings.allowContentAccess = false
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
    show(view)
    web = view
    watchdog.postDelayed({ offline(connection) }, LOAD_TIMEOUT)
    view.loadUrl(connection.page)
  }

  companion object {
    private const val LOAD_TIMEOUT = 8000L
  }
}
