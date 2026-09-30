package sh.cydonia.remote

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.RingtoneManager
import android.os.IBinder
import java.net.HttpURLConnection
import java.net.URL

class NoticeService : Service() {
  @Volatile private var running = false
  private var worker: Thread? = null

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    createChannels()
    startForeground(WATCHING_ID, watching())
    if (!running) {
      running = true
      worker = Thread(::listen, "cydonia-notices").also { it.start() }
    }
    return START_STICKY
  }

  override fun onDestroy() {
    running = false
    worker?.interrupt()
    super.onDestroy()
  }

  private fun listen() {
    var wait = RETRY_FIRST
    while (running) {
      val connection = Connection.load(this)
      val heard = if (connection.complete && connection.private) runCatching { poll(connection) }.getOrNull() else null
      if (heard == null) {
        if (!pause(wait)) return
        wait = (wait * 2).coerceAtMost(RETRY_LAST)
        continue
      }
      wait = RETRY_FIRST
      remember(connection, heard.latest)
      if (!MainActivity.visible) heard.notices.forEach(::announce)
    }
  }

  private fun pause(millis: Long): Boolean =
    try {
      Thread.sleep(millis)
      true
    } catch (_: InterruptedException) {
      false
    }

  private fun cursor(connection: Connection): Long? {
    val store = getSharedPreferences(STORE, Context.MODE_PRIVATE)
    return if (store.contains(connection.address)) store.getLong(connection.address, 0) else null
  }

  private fun remember(connection: Connection, latest: Long) {
    getSharedPreferences(STORE, Context.MODE_PRIVATE).edit().putLong(connection.address, latest).apply()
  }

  private fun poll(connection: Connection): Heard {
    val after = cursor(connection)?.let { "?after=$it&wait=$POLL_SECONDS" }.orEmpty()
    val link = URL("${connection.base}/v1/notices$after").openConnection() as HttpURLConnection
    try {
      link.connectTimeout = CONNECT_TIMEOUT
      link.readTimeout = (POLL_SECONDS + 15) * 1000
      link.setRequestProperty("Authorization", "Bearer ${connection.token}")
      check(link.responseCode == HttpURLConnection.HTTP_OK) { "notices answered ${link.responseCode}" }
      return Notices.parse(link.inputStream.bufferedReader().use { it.readText() })
    } finally {
      link.disconnect()
    }
  }

  private fun createChannels() {
    val manager = getSystemService(NotificationManager::class.java)
    RETIRED_CHANNELS.forEach(manager::deleteNotificationChannel)
    manager.createNotificationChannel(
      NotificationChannel(CHANNEL_WATCHING, getString(R.string.channel_watching), NotificationManager.IMPORTANCE_MIN),
    )
    listOf(
      Triple(CHANNEL_APPROVALS, R.string.channel_approvals, NotificationManager.IMPORTANCE_HIGH),
      Triple(CHANNEL_DONE, R.string.channel_done, NotificationManager.IMPORTANCE_HIGH),
      Triple(CHANNEL_LOST, R.string.channel_lost, NotificationManager.IMPORTANCE_HIGH),
    ).forEach { (id, name, importance) ->
      manager.createNotificationChannel(loud(NotificationChannel(id, getString(name), importance), id))
    }
  }

  private fun loud(channel: NotificationChannel, id: String): NotificationChannel =
    channel.apply {
      setSound(
        RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION),
        AudioAttributes.Builder()
          .setUsage(AudioAttributes.USAGE_NOTIFICATION)
          .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
          .build(),
      )
      enableVibration(true)
      vibrationPattern = if (id == CHANNEL_APPROVALS) ASKING_BUZZ else DONE_BUZZ
      lockscreenVisibility = Notification.VISIBILITY_PUBLIC
    }

  private fun opener(): PendingIntent =
    PendingIntent.getActivity(
      this,
      0,
      Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

  private fun watching(): Notification =
    Notification.Builder(this, CHANNEL_WATCHING)
      .setSmallIcon(android.R.drawable.stat_notify_sync_noanim)
      .setContentTitle(getString(R.string.watching_title))
      .setContentText(getString(R.string.watching_body))
      .setContentIntent(opener())
      .setOngoing(true)
      .build()

  private fun channelFor(kind: String): String =
    when (kind) {
      "approval" -> CHANNEL_APPROVALS
      "lost" -> CHANNEL_LOST
      else -> CHANNEL_DONE
    }

  private fun announce(notice: Notice) {
    val built = Notification.Builder(this, channelFor(notice.kind))
      .setSmallIcon(android.R.drawable.stat_notify_chat)
      .setContentTitle(notice.title)
      .setContentText(notice.body)
      .setStyle(Notification.BigTextStyle().bigText(notice.body))
      .setContentIntent(opener())
      .setAutoCancel(true)
      .build()
    getSystemService(NotificationManager::class.java).notify(notice.record, notice.kind.hashCode(), built)
  }

  companion object {
    private const val STORE = "notices"
    private const val WATCHING_ID = 1
    private const val CHANNEL_WATCHING = "watching"
    private const val CHANNEL_APPROVALS = "approvals_loud"
    private const val CHANNEL_DONE = "done_loud"
    private const val CHANNEL_LOST = "lost_loud"
    private val RETIRED_CHANNELS = listOf("approvals", "done", "lost")
    private val ASKING_BUZZ = longArrayOf(0, 300, 150, 300, 150, 300)
    private val DONE_BUZZ = longArrayOf(0, 250, 120, 250)
    private const val POLL_SECONDS = 25
    private const val CONNECT_TIMEOUT = 10_000
    private const val RETRY_FIRST = 2_000L
    private const val RETRY_LAST = 30_000L

    fun start(context: Context) {
      context.startForegroundService(Intent(context, NoticeService::class.java))
    }
  }
}
