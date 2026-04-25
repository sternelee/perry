package com.perry.app

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat

/**
 * Fires when an `AlarmManager` alarm scheduled by `notificationSchedule`
 * (#96) goes off. Builds and posts the notification with the same channel
 * + tap-PendingIntent setup `PerryBridge.sendNotification` uses, so taps
 * route through `PerryNotificationReceiver` → JS callback (#97).
 *
 * Registered in AndroidManifest under action `com.perry.app.SCHEDULED_FIRE`.
 * Intent extras: `id` (string), `title`, `body`.
 */
class PerryScheduledNotificationReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val id = intent.getStringExtra("id") ?: run {
            Log.w("PerryScheduled", "scheduled fire missing id extra")
            return
        }
        val title = intent.getStringExtra("title") ?: ""
        val body = intent.getStringExtra("body") ?: ""

        val notificationManager = NotificationManagerCompat.from(context)

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                "perry-default",
                "Notifications",
                NotificationManager.IMPORTANCE_DEFAULT
            )
            notificationManager.createNotificationChannel(channel)
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (ContextCompat.checkSelfPermission(
                    context,
                    Manifest.permission.POST_NOTIFICATIONS
                ) != PackageManager.PERMISSION_GRANTED
            ) {
                Log.w("PerryScheduled", "POST_NOTIFICATIONS not granted; alarm fire dropped")
                return
            }
        }

        // Tap routing — same shape as sendNotification's PendingIntent.
        val tapIntent = Intent(context, PerryNotificationReceiver::class.java).apply {
            action = "com.perry.app.NOTIFICATION_TAP"
            putExtra("id", id)
        }
        val tapPending = PendingIntent.getBroadcast(
            context,
            id.hashCode(),
            tapIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val notification = NotificationCompat.Builder(context, "perry-default")
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle(title)
            .setContentText(body)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setAutoCancel(true)
            .setContentIntent(tapPending)
            .build()

        try {
            notificationManager.notify(id.hashCode(), notification)
        } catch (e: SecurityException) {
            Log.w("PerryScheduled", "scheduled notify dropped: ${e.message}", e)
        }
    }
}
