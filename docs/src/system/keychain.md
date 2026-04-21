# Keychain

Securely store sensitive data like tokens, passwords, and API keys using the platform's secure storage.

## Usage

```typescript,no-test
import { keychainSet, keychainGet } from "perry/system";

// Store a secret
keychainSet("api-token", "sk-abc123...");

// Retrieve a secret
const token = keychainGet("api-token");
```

## Platform Storage

| Platform | Backend |
|----------|---------|
| macOS | Security.framework (Keychain) |
| iOS | Security.framework (Keychain) |
| Android | Android Keystore |
| Windows | Windows Credential Manager (CredWrite/CredRead/CredDelete) |
| Linux | libsecret |
| Web | localStorage (not truly secure) |

> **Web**: The web platform uses `localStorage`, which is not encrypted. For web apps handling sensitive data, consider server-side storage instead.

## Next Steps

- [Preferences](preferences.md) — Non-sensitive preferences
- [Notifications](notifications.md) — Local notifications
- [Overview](overview.md) — All system APIs
