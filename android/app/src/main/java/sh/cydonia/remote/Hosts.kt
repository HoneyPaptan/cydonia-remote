package sh.cydonia.remote

import android.content.Context
import android.webkit.JavascriptInterface
import org.json.JSONArray
import org.json.JSONObject
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors

class Hosts(private val activity: MainActivity, private val current: Connection) {
  private val probes = ConcurrentHashMap<String, Pair<Boolean, Long>>()
  private val probing = ConcurrentHashMap.newKeySet<String>()
  private val worker = Executors.newCachedThreadPool()

  @JavascriptInterface
  fun list(): String {
    val shown = JSONArray()
    for (saved in saved(activity, current)) {
      val entry = JSONObject().put("address", saved.address).put("current", saved.address == current.address)
      reachable(saved)?.let { entry.put("reachable", it) }
      shown.put(entry)
    }
    return shown.toString()
  }

  @JavascriptInterface
  fun add(address: String, token: String) {
    val added = Connection(address.trim(), token.trim())
    if (added.complete) remember(activity, added)
  }

  @JavascriptInterface
  fun remove(address: String) {
    if (address == current.address) return
    forget(activity, address)
  }

  @JavascriptInterface
  fun open(address: String) {
    val chosen = saved(activity, current).firstOrNull { it.address == address } ?: return
    activity.runOnUiThread { activity.switchTo(chosen) }
  }

  private fun reachable(connection: Connection): Boolean? {
    val known = probes[connection.address]
    val stale = known == null || System.currentTimeMillis() - known.second > PROBE_AGE
    if (stale && probing.add(connection.address)) {
      worker.execute {
        val up = runCatching { probe(connection) }.getOrDefault(false)
        probes[connection.address] = up to System.currentTimeMillis()
        probing.remove(connection.address)
      }
    }
    return known?.first
  }

  private fun probe(connection: Connection): Boolean {
    val uri = android.net.Uri.parse(connection.base)
    val port = if (uri.port > 0) uri.port else DEFAULT_PORT
    Socket().use { socket ->
      socket.connect(InetSocketAddress(uri.host, port), PROBE_TIMEOUT)
    }
    return true
  }

  companion object {
    private const val STORE = "hosts"
    private const val LIST = "saved"
    private const val PROBE_AGE = 5000L
    private const val PROBE_TIMEOUT = 1500
    private const val DEFAULT_PORT = 80

    fun saved(context: Context, current: Connection? = null): List<Connection> {
      val text = context.getSharedPreferences(STORE, Context.MODE_PRIVATE).getString(LIST, "[]").orEmpty()
      val found = runCatching {
        val array = JSONArray(text)
        (0 until array.length()).map { index ->
          val entry = array.getJSONObject(index)
          Connection(entry.optString("address"), entry.optString("token"))
        }
      }.getOrDefault(emptyList()).filter { it.complete }
      val head = listOfNotNull(current?.takeIf { it.complete })
      return head + found.filter { saved -> head.none { it.address == saved.address } }
    }

    fun remember(context: Context, connection: Connection) {
      write(context, listOf(connection) + saved(context).filter { it.address != connection.address })
    }

    fun forget(context: Context, address: String) {
      write(context, saved(context).filter { it.address != address })
    }

    private fun write(context: Context, hosts: List<Connection>) {
      val array = JSONArray()
      hosts.forEach { array.put(JSONObject().put("address", it.address).put("token", it.token)) }
      context.getSharedPreferences(STORE, Context.MODE_PRIVATE).edit().putString(LIST, array.toString()).apply()
    }
  }
}
