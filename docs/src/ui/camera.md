# Camera

The `perry/ui` module provides a live camera preview widget with color sampling capabilities.

```typescript,no-test
import { CameraView, cameraStart, cameraStop, cameraFreeze, cameraUnfreeze, cameraSampleColor, cameraSetOnTap } from "perry/ui";
```

> **Platform support:** iOS only. Other platforms are planned.

## Quick Example

```typescript,no-test
import { App, VStack, Text, State } from "perry/ui";
import { CameraView, cameraStart, cameraStop, cameraSampleColor, cameraSetOnTap } from "perry/ui";

const colorHex = State("#000000");

const cam = CameraView();
cameraStart(cam);

cameraSetOnTap(cam, (x, y) => {
  const rgb = cameraSampleColor(x, y);
  if (rgb >= 0) {
    const r = Math.floor(rgb / 65536);
    const g = Math.floor((rgb % 65536) / 256);
    const b = Math.floor(rgb % 256);
    colorHex.set(`#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${b.toString(16).padStart(2, "0")}`);
  }
});

App({
  title: "Color Picker",
  width: 400,
  height: 600,
  body: VStack(16, [
    cam,
    Text(`Color: ${colorHex.value}`),
  ]),
});
```

## API Reference

### `CameraView()`

Create a live camera preview widget.

```typescript,no-test
const cam = CameraView();
```

Returns a widget handle. The camera does not start automatically — call `cameraStart()` to begin capture.

### `cameraStart(handle)`

Start the live camera feed.

```typescript,no-test
cameraStart(cam);
```

On iOS, the camera permission dialog is shown automatically on first use.

### `cameraStop(handle)`

Stop the camera feed and release the capture session.

```typescript,no-test
cameraStop(cam);
```

### `cameraFreeze(handle)`

Pause the live preview (freeze the current frame).

```typescript,no-test
cameraFreeze(cam);
```

The camera session remains active but the preview stops updating. Useful for "capture" moments where you want to inspect the frozen frame.

### `cameraUnfreeze(handle)`

Resume the live preview after a freeze.

```typescript,no-test
cameraUnfreeze(cam);
```

### `cameraSampleColor(x, y)`

Sample the pixel color at normalized coordinates.

```typescript,no-test
const rgb = cameraSampleColor(0.5, 0.5); // center of frame
```

- `x`, `y` are normalized coordinates (0.0–1.0)
- Returns packed RGB as a number: `r * 65536 + g * 256 + b`
- Returns `-1` if no frame is available

To extract individual channels:

```typescript,no-test
const r = Math.floor(rgb / 65536);
const g = Math.floor((rgb % 65536) / 256);
const b = Math.floor(rgb % 256);
```

The color is averaged over a 5x5 pixel region around the sample point for noise reduction.

### `cameraSetOnTap(handle, callback)`

Register a tap handler on the camera view.

```typescript,no-test
cameraSetOnTap(cam, (x, y) => {
  // x, y are normalized coordinates (0.0-1.0)
  const rgb = cameraSampleColor(x, y);
});
```

The callback receives normalized coordinates of the tap location, which can be passed directly to `cameraSampleColor()`.

## Implementation

On iOS, the camera uses AVCaptureSession with AVCaptureVideoPreviewLayer for GPU-accelerated live preview, and AVCaptureVideoDataOutput for frame capture. Color sampling reads pixel data from CVPixelBuffer.

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Audio Capture](../system/audio.md) — Microphone input and sound metering
