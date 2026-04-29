# Notifications

Send local notifications using the platform's notification system. Every
snippet below is excerpted from
[`docs/examples/system/snippets.ts`](../../examples/system/snippets.ts) — CI
links it on every PR.

## Sending a notification

```typescript
{{#include ../../examples/system/snippets.ts:notification-send}}
```

## Reacting to a tap

```typescript
{{#include ../../examples/system/snippets.ts:notification-tap}}
```

`action` is the action-button identifier when the user picks a button, or
`undefined` for the default banner tap.

## Cancelling a scheduled notification

```typescript
{{#include ../../examples/system/snippets.ts:notification-cancel}}
```

`notificationCancel(id)` is a no-op if no scheduled notification with that id
exists.

## Push notifications (APNs / Firebase)

```typescript
{{#include ../../examples/system/snippets.ts:notification-remote}}
```

`notificationRegisterRemote(cb)` fires once when the OS returns a device token
— on Apple platforms the token is the canonical uppercase hex string APNs
expects. `notificationOnReceive(cb)` runs whenever a remote payload arrives
while the app is foregrounded; the payload is the APNs `aps` userInfo
dictionary (or equivalent platform shape) converted to a plain object.

Requires the relevant platform capability (APNs entitlement on iOS/macOS,
Firebase Messaging on Android — wired via JNI through
`PerryFirebaseMessagingService`, see [#98](https://github.com/PerryTS/perry/issues/98)).
No-op on platforms without a push pipeline (tvOS, visionOS, watchOS, GTK4,
Windows, Web).

## Platform Implementation

| Platform | Backend |
|----------|---------|
| macOS | UNUserNotificationCenter |
| iOS | UNUserNotificationCenter |
| Android | NotificationManager |
| Windows | Toast notifications |
| Linux | GNotification |
| Web | Web Notification API |

> **Permissions**: On macOS, iOS, and Web, the user may need to grant
> notification permissions. On first use, the system will prompt automatically.

## Next Steps

- [Keychain](keychain.md) — Secure storage
- [Other](other.md) — Additional system APIs
- [Overview](overview.md) — All system APIs
