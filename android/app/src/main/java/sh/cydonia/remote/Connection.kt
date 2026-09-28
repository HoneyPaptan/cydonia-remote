package sh.cydonia.remote

import android.content.Context
import android.net.Uri

data class Connection(val address: String, val token: String) {
  val complete: Boolean
    get() = address.isNotBlank() && token.isNotBlank()

  val base: String
    get() = if (address.startsWith("http")) address.trimEnd('/') else "http://${address.trimEnd('/')}"

  val page: String
    get() = "$base/#token=$token"

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

    fun from(link: Uri?): Connection? {
      if (link?.scheme != "cydonia" || link.host != "connect") return null
      val found = Connection(link.getQueryParameter("address").orEmpty(), link.getQueryParameter("token").orEmpty())
      return found.takeIf { it.complete }
    }
  }
}
