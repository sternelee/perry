# Canvas

The `Canvas` widget provides a 2D drawing surface for custom graphics.

> **Availability**: `Canvas` is wired in the **LLVM** codegen path (macOS, iOS,
> Linux, Android) and the **JS / web / wasm** codegen path. Closed via
> [#190](https://github.com/PerryTS/perry/issues/190). The snippets below are
> still kept as `text` fences pending an end-to-end doc-tests example that
> attaches the canvas to a run loop and verifies pixel output; they compile
> and link cleanly today.

The drawing API is **method-based** on the canvas handle (matching the FFI
shape — `perry_ui_canvas_set_fill_color(handle, r, g, b, a)` etc.). Colors
are RGBA floats in `[0.0, 1.0]`.

## Creating a Canvas

```text
import { Canvas } from "perry/ui";

const canvas = Canvas(400, 300);
canvas.setFillColor(1.0, 0.4, 0.0, 1.0);
canvas.fillRect(10, 10, 100, 80);
```

`Canvas(width, height)` creates a canvas widget; subsequent draw operations
are method calls on the returned handle.

## Drawing Shapes

### Rectangles

```text
canvas.setFillColor(1.0, 0.0, 0.0, 1.0);    // red
canvas.fillRect(10, 10, 100, 80);

canvas.setStrokeColor(0.0, 0.0, 1.0, 1.0);  // blue
canvas.setLineWidth(2);
canvas.strokeRect(150, 10, 100, 80);
```

### Lines

```text
canvas.setStrokeColor(0.0, 0.0, 0.0, 1.0);
canvas.setLineWidth(1);
canvas.beginPath();
canvas.moveTo(10, 10);
canvas.lineTo(200, 150);
canvas.stroke();
```

### Circles and Arcs

```text
canvas.setFillColor(0.0, 1.0, 0.0, 1.0);
canvas.beginPath();
canvas.arc(200, 150, 50, 0, Math.PI * 2);  // x, y, radius, startAngle, endAngle
canvas.fill();
```

### Text

```text
canvas.setFillColor(0.0, 0.0, 0.0, 1.0);
canvas.setFont("16px sans-serif");
canvas.fillText("Hello Canvas!", 50, 50);
```

## Platform Notes

| Platform | Implementation | Status |
|----------|---------------|--------|
| Web | HTML5 Canvas | Wired |
| WASM | HTML5 Canvas via JS bridge | Wired |
| macOS | Core Graphics (CGContext) | Wired |
| iOS | Core Graphics (CGContext) | Wired |
| Linux | Cairo | Wired |
| Windows | GDI | Planned |
| Android | Canvas/Bitmap | Wired |

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Animation](animation.md) — Animating widget properties
- [Styling](styling.md) — Widget styling
