# Notifications

Send local notifications using the platform's notification system.

## Usage

```typescript,no-test
import { sendNotification } from "perry/system";

sendNotification("Download Complete", "Your file has been downloaded successfully.");
```

## Platform Implementation

| Platform | Backend |
|----------|---------|
| macOS | UNUserNotificationCenter |
| iOS | UNUserNotificationCenter |
| Android | NotificationManager |
| Windows | Toast notifications |
| Linux | GNotification |
| Web | Web Notification API |

> **Permissions**: On macOS, iOS, and Web, the user may need to grant notification permissions. On first use, the system will prompt for permission automatically.

## Next Steps

- [Keychain](keychain.md) — Secure storage
- [Other](other.md) — Additional system APIs
- [Overview](overview.md) — All system APIs
