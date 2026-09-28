package sh.cydonia.remote

import android.content.Context
import android.net.Uri
import java.net.Inet4Address
import java.net.InetAddress

data class Connection(val address: String, val token: String) {
  val complete: Boolean
    get() = address.isNotBlank() && token.isNotBlank()

  val base: String
    get() = if (address.startsWith("http")) address.trimEnd('/') else "http://${address.trimEnd('/')}"

  val page: String
    get() = "$base/#token=$token"

  val host: String
    get() = Uri.parse(base).host.orEmpty()

  val private: Boolean
    get() = base.startsWith("https://") || tailnet(host)

  fun save(context: Context) {
    context.getSharedPreferences(STORE, Context.MODE_PRIVATE)
      .edit()
      .putString(ADDRESS, address.trim())
      .putString(TOKEN, token.trim())
      .apply()
  }

  companion object {
    private const val STORE = "connection"
    private const val ADDRESS = "address"
    private const val TOKEN = "token"

    fun load(context: Context): Connection {
      val store = context.getSharedPreferences(STORE, Context.MODE_PRIVATE)
      return Connection(store.getString(ADDRESS, "").orEmpty(), store.getString(TOKEN, "").orEmpty())
    }

    private const val TAILNET_SUFFIX = ".ts.net"

    fun tailnet(host: String): Boolean {
      if (host.isBlank()) return false
      if (host == "localhost" || host.endsWith(TAILNET_SUFFIX)) return true
      if (!host.all { it.isDigit() || it == '.' }) return false
      val address = runCatching { InetAddress.getByName(host) }.getOrNull() as? Inet4Address ?: return false
      val bytes = address.address.map { it.toInt() and 0xff }
      return address.isLoopbackAddress || (bytes[0] == 100 && bytes[1] in 64..127)
    }

    fun from(link: Uri?): Connection? {
      if (link?.scheme != "cydonia" || link.host != "connect") return null
      val found = Connection(link.getQueryParameter("address").orEmpty(), link.getQueryParameter("token").orEmpty())
      return found.takeIf { it.complete }
    }
  }
}
