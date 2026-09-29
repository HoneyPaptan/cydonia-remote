package sh.cydonia.remote

import org.json.JSONObject

data class Notice(
  val id: Long,
  val kind: String,
  val project: String,
  val record: String,
  val title: String,
  val body: String,
)

data class Heard(val latest: Long, val notices: List<Notice>)

object Notices {
  fun parse(text: String): Heard {
    val whole = JSONObject(text)
    val array = whole.getJSONArray("notices")
    val notices = (0 until array.length()).map { index ->
      val entry = array.getJSONObject(index)
      Notice(
        id = entry.getLong("id"),
        kind = entry.getString("kind"),
        project = entry.optString("project"),
        record = entry.optString("record"),
        title = entry.optString("title"),
        body = entry.optString("body"),
      )
    }
    return Heard(whole.getLong("latest"), notices)
  }
}
