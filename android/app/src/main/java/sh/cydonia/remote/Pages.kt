package sh.cydonia.remote

import android.app.Activity
import android.graphics.Color
import android.view.View
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import org.json.JSONObject

class Pages(private val activity: Activity, private val layer: FrameLayout) {
  private val views = HashMap<Long, WebView>()
  private val places = HashMap<Long, List<Int>>()

  @JavascriptInterface
  fun page(message: String) {
    val op = runCatching { JSONObject(message) }.getOrNull() ?: return
    activity.runOnUiThread { handle(op) }
  }

  fun back(): Boolean {
    val shown = views.values.firstOrNull { it.visibility == View.VISIBLE && it.canGoBack() } ?: return false
    shown.goBack()
    return true
  }

  fun clear() {
    views.values.forEach { layer.removeView(it); it.destroy() }
    views.clear()
    places.clear()
  }

  private fun handle(op: JSONObject) {
    val id = op.optLong("id")
    when (op.optString("op")) {
      "place" -> place(id, op)
      "park" -> views[id]?.visibility = View.GONE
      "load" -> views[id]?.loadUrl(op.optString("url"))
      "back" -> views[id]?.let { if (it.canGoBack()) it.goBack() }
      "forward" -> views[id]?.let { if (it.canGoForward()) it.goForward() }
      "reload" -> views[id]?.reload()
      "close" -> {
        views.remove(id)?.let { layer.removeView(it); it.destroy() }
        places.remove(id)
      }
    }
  }

  private fun place(id: Long, op: JSONObject) {
    val view = views.getOrPut(id) { make(op.optString("url")) }
    val box = listOf("x", "y", "width", "height").map { op.optDouble(it).toInt() }
    if (places[id] != box) {
      places[id] = box
      view.layoutParams = FrameLayout.LayoutParams(box[2], box[3]).apply {
        leftMargin = box[0]
        topMargin = box[1]
      }
    }
    view.visibility = View.VISIBLE
  }

  private fun make(url: String): WebView {
    val view = WebView(activity).apply {
      setBackgroundColor(Color.WHITE)
      settings.javaScriptEnabled = true
      settings.domStorageEnabled = true
      settings.allowFileAccess = false
      settings.allowContentAccess = false
      webViewClient = WebViewClient()
      visibility = View.GONE
    }
    layer.addView(view, FrameLayout.LayoutParams(0, 0))
    view.loadUrl(url)
    return view
  }
}
