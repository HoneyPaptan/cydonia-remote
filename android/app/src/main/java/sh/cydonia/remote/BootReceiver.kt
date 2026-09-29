package sh.cydonia.remote

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class BootReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    if (Connection.load(context).complete) NoticeService.start(context)
  }
}
