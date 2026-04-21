# Audio Capture

The `perry/system` module provides real-time audio capture from the device microphone, with A-weighted dB(A) level metering and waveform sampling — everything needed to build a sound meter, audio visualizer, or voice-level indicator.

```typescript,no-test
import { audioStart, audioStop, audioGetLevel, audioGetPeak, audioGetWaveformSamples } from "perry/system";
```

## Quick Example

```typescript,no-test
import { App, Text, VStack, State, Canvas } from "perry/ui";
import { audioStart, audioStop, audioGetLevel, audioGetPeak, audioGetWaveformSamples } from "perry/system";

audioStart();

const db = State(0);

// Poll the level every 100ms
setInterval(() => {
  db.set(audioGetLevel());
}, 100);

App({
  title: "Sound Meter",
  width: 400,
  height: 300,
  body: VStack(16, [
    Text(`${db.value} dB`),
  ]),
});
```

## API Reference

### `audioStart()`

Start capturing audio from the device microphone.

```typescript,no-test
const ok = audioStart(); // 1 = success, 0 = failure
```

On platforms that require permission (iOS, Android, Web), the system permission dialog is shown automatically. Returns `1` on success, `0` on failure (e.g., permission denied, no microphone).

### `audioStop()`

Stop audio capture and release the microphone.

```typescript,no-test
audioStop();
```

### `audioGetLevel()`

Get the current A-weighted sound level in dB(A).

```typescript,no-test
const db = audioGetLevel(); // e.g. 45.2
```

Returns a smoothed dB(A) value (EMA with 125ms time constant). Typical ranges:
- ~30 dB — quiet room
- ~50 dB — normal conversation
- ~70 dB — busy street
- ~90 dB — loud music
- ~110+ dB — dangerously loud

### `audioGetPeak()`

Get the current peak sample amplitude.

```typescript,no-test
const peak = audioGetPeak(); // 0.0 to 1.0
```

Returns a normalized amplitude value (0.0 = silence, 1.0 = clipping). Useful for simple level indicators without dB conversion.

### `audioGetWaveformSamples(count)`

Get recent dB samples for waveform visualization.

```typescript,no-test
const samples = audioGetWaveformSamples(64); // array of up to 64 dB values
```

Returns an array of recent dB(A) readings from a 256-sample ring buffer. Pass the number of samples you want (max 256). Useful for drawing waveform displays or level history charts.

### `getDeviceModel()`

Get the device model identifier.

```typescript,no-test
import { getDeviceModel } from "perry/system";

const model = getDeviceModel(); // e.g. "MacBookPro18,3", "iPhone15,2"
```

## Platform Implementations

| Platform | Audio Backend | Permissions |
|----------|--------------|-------------|
| macOS | AVAudioEngine | Microphone permission dialog |
| iOS | AVAudioSession + AVAudioEngine | System permission dialog |
| Android | AudioRecord (JNI) | RECORD_AUDIO permission |
| Linux | PulseAudio (libpulse-simple) | None (system-level) |
| Windows | WASAPI (shared mode) | None |
| Web | getUserMedia + AnalyserNode | Browser permission dialog |

All platforms capture at 48kHz mono and apply the same A-weighting filter (IEC 61672 standard, 3 cascaded biquad sections).

## Next Steps

- [Camera](../ui/camera.md) — Live camera preview (iOS)
- [Overview](overview.md) — All system APIs
