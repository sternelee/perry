// Type declarations for perry/system — Perry's platform & system APIs
// These types are auto-written by `perry init` / `perry types` so IDEs
// and tsc can resolve `import { ... } from "perry/system"`.

// ---------------------------------------------------------------------------
// Theme & Device
// ---------------------------------------------------------------------------

/** Returns true if the system is in dark mode. */
export function isDarkMode(): boolean;

/** Returns the device idiom (e.g. "phone", "pad", "mac", "tv"). */
export function getDeviceIdiom(): string;

/** Returns the device model identifier (e.g. "iPhone13,4"). */
export function getDeviceModel(): string;

// ---------------------------------------------------------------------------
// URL
// ---------------------------------------------------------------------------

/** Open a URL in the default browser or system handler. */
export function openURL(url: string): void;

// ---------------------------------------------------------------------------
// Keychain (secure credential storage)
// ---------------------------------------------------------------------------

/** Save a value to the system keychain. */
export function keychainSave(key: string, value: string): void;

/** Retrieve a value from the system keychain. */
export function keychainGet(key: string): string;

/** Delete a value from the system keychain. */
export function keychainDelete(key: string): void;

// ---------------------------------------------------------------------------
// User Preferences (persistent key-value storage)
// ---------------------------------------------------------------------------

/**
 * Read a preference value. Returns the stored string or number, or `undefined`
 * if the key is absent. The runtime branches on the NaN-box tag of the stored
 * NSUserDefaults entry, so callers see the original type back.
 */
export function preferencesGet(key: string): string | number | undefined;

/**
 * Write a preference value. Strings and numbers are stored natively via
 * NSUserDefaults / GSettings / the Windows registry depending on platform;
 * the same value round-trips through `preferencesGet`.
 */
export function preferencesSet(key: string, value: string | number): void;

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

/** Send a local notification. */
export function notificationSend(title: string, body: string): void;

/**
 * Register for remote (push) notifications.
 *
 * The callback fires once when the OS returns a device token. On Apple
 * platforms the token is formatted as the canonical uppercase hex string
 * (no spaces, no `<>`) that APNs-side code expects.
 *
 * Requires the relevant platform capability:
 * - iOS/macOS: APNs entitlement (`aps-environment`) + a provisioning profile.
 * - Android: Firebase Messaging + `google-services.json` (not yet wired).
 *
 * No-op on platforms without a push pipeline (tvOS, visionOS, watchOS, GTK4,
 * Windows, Web).
 */
export function notificationRegisterRemote(onToken: (token: string) => void): void;

/**
 * Register a handler for incoming remote-notification payloads received while
 * the app is foregrounded. The payload object is the APNs `aps` userInfo
 * dictionary (or equivalent platform shape) converted to a plain object.
 *
 * Background/terminated-app delivery is a separate pipeline (see issue #98).
 */
export function notificationOnReceive(cb: (payload: object) => void): void;

// ---------------------------------------------------------------------------
// Audio input
// ---------------------------------------------------------------------------

/** Start audio capture. Returns 1 on success, 0 on failure. */
export function audioStart(): number;

/** Stop audio capture. */
export function audioStop(): void;

/** Get the current audio input level (0-1). */
export function audioGetLevel(): number;

/** Get the peak audio input level (0-1). */
export function audioGetPeak(): number;

/** Get waveform data with the given number of samples. */
export function audioGetWaveform(sampleCount: number): number;
