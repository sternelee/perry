package com.perry.app

import android.util.Log
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import org.json.JSONObject

/**
 * Routes FCM events back to the Perry runtime (#95).
 *
 * Registered in `AndroidManifest.xml` for the
 * `com.google.firebase.MESSAGING_EVENT` action that FCM dispatches.
 *
 * - `onNewToken` fires once per registration (and on every rotation
 *   thereafter) → forwards the token string to native via
 *   `PerryBridge.nativeNotificationToken`, which dispatches to the JS
 *   closure registered with `notificationRegisterRemote`.
 * - `onMessageReceived` fires for every push payload while the app is
 *   foregrounded → serializes the data + notification fields to JSON and
 *   forwards via `PerryBridge.nativeNotificationReceive`. Background
 *   delivery (`fetchCompletionHandler:` equivalent) is `#98` territory and
 *   isn't wired here.
 */
class PerryFirebaseMessagingService : FirebaseMessagingService() {
    override fun onNewToken(token: String) {
        super.onNewToken(token)
        try {
            PerryBridge.nativeNotificationToken(token)
        } catch (e: UnsatisfiedLinkError) {
            // Native lib not loaded — process is cold-started just for
            // this broadcast. #98 territory; drop with a log.
            Log.w("PerryFirebase", "nativeNotificationToken unavailable", e)
        }
    }

    override fun onMessageReceived(message: RemoteMessage) {
        super.onMessageReceived(message)
        val json = remoteMessageToJson(message).toString()
        try {
            PerryBridge.nativeNotificationReceive(json)
        } catch (e: UnsatisfiedLinkError) {
            Log.w("PerryFirebase", "nativeNotificationReceive unavailable", e)
        }
    }

    private fun remoteMessageToJson(message: RemoteMessage): JSONObject {
        val obj = JSONObject()
        message.from?.let { obj.put("from", it) }
        message.messageId?.let { obj.put("messageId", it) }
        message.messageType?.let { obj.put("messageType", it) }
        obj.put("sentTime", message.sentTime)
        obj.put("ttl", message.ttl)

        // `data` map (custom key/value payload sent from your server).
        if (message.data.isNotEmpty()) {
            val dataObj = JSONObject()
            for ((k, v) in message.data) {
                dataObj.put(k, v)
            }
            obj.put("data", dataObj)
        }

        // `notification` block (when the server sent the
        // notification-shape payload as opposed to data-only).
        message.notification?.let { n ->
            val notif = JSONObject()
            n.title?.let { notif.put("title", it) }
            n.body?.let { notif.put("body", it) }
            n.tag?.let { notif.put("tag", it) }
            n.color?.let { notif.put("color", it) }
            n.icon?.let { notif.put("icon", it) }
            n.sound?.let { notif.put("sound", it) }
            n.clickAction?.let { notif.put("clickAction", it) }
            n.channelId?.let { notif.put("channelId", it) }
            obj.put("notification", notif)
        }
        return obj
    }
}
