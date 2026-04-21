# Preferences

Store and retrieve user preferences using the platform's native storage.

## Usage

```typescript,no-test
import { preferencesSet, preferencesGet } from "perry/system";

// Store a preference
preferencesSet("username", "perry");
preferencesSet("fontSize", "14");
preferencesSet("darkMode", "true");

// Read a preference
const username = preferencesGet("username");  // "perry"
const fontSize = preferencesGet("fontSize");  // "14"
```

Values are stored as strings. Convert numbers and booleans as needed:

```typescript,no-test
preferencesSet("count", String(42));
const count = Number(preferencesGet("count"));
```

## Platform Storage

| Platform | Backend |
|----------|---------|
| macOS | NSUserDefaults |
| iOS | NSUserDefaults |
| Android | SharedPreferences |
| Windows | Windows Registry |
| Linux | GSettings / file-based |
| Web | localStorage |

Preferences persist across app launches. They are not encrypted — use [Keychain](keychain.md) for sensitive data.

## Next Steps

- [Keychain](keychain.md) — Secure storage
- [Overview](overview.md) — All system APIs
