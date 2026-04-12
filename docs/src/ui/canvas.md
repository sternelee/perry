# Canvas

The `Canvas` widget provides a 2D drawing surface for custom graphics.

## Creating a Canvas

```typescript
import { Canvas } from "perry/ui";

const canvas = Canvas(400, 300, (ctx) => {
  // Drawing code here
  ctx.fillRect(10, 10, 100, 80);
});
```

`Canvas(width, height, drawCallback)` creates a canvas and calls your drawing function.

## Drawing Shapes

### Rectangles

```typescript
Canvas(400, 300, (ctx) => {
  // Filled rectangle
  ctx.setFillColor("#FF0000");
  ctx.fillRect(10, 10, 100, 80);

  // Stroked rectangle
  ctx.setStrokeColor("#0000FF");
  ctx.setLineWidth(2);
  ctx.strokeRect(150, 10, 100, 80);
});
```

### Lines

```typescript
Canvas(400, 300, (ctx) => {
  ctx.setStrokeColor("#000000");
  ctx.setLineWidth(1);
  ctx.beginPath();
  ctx.moveTo(10, 10);
  ctx.lineTo(200, 150);
  ctx.stroke();
});
```

### Circles and Arcs

```typescript
Canvas(400, 300, (ctx) => {
  ctx.setFillColor("#00FF00");
  ctx.beginPath();
  ctx.arc(200, 150, 50, 0, Math.PI * 2); // x, y, radius, startAngle, endAngle
  ctx.fill();
});
```

## Colors

```typescript
Canvas(400, 300, (ctx) => {
  ctx.setFillColor("#FF6600");    // Hex color
  ctx.setStrokeColor("#333333");
  ctx.setLineWidth(3);
});
```

## Gradients

```typescript
Canvas(400, 300, (ctx) => {
  ctx.setGradient("#FF0000", "#0000FF"); // Start color, end color
  ctx.fillRect(0, 0, 400, 300);
});
```

## Text on Canvas

```typescript
Canvas(400, 300, (ctx) => {
  ctx.setFillColor("#000000");
  ctx.fillText("Hello Canvas!", 50, 50);
});
```

## Platform Notes

| Platform | Implementation |
|----------|---------------|
| macOS | Core Graphics (CGContext) |
| iOS | Core Graphics (CGContext) |
| Linux | Cairo |
| Windows | GDI |
| Android | Canvas/Bitmap |
| Web | HTML5 Canvas |

## Complete Example

```typescript
import { App, Canvas, VStack } from "perry/ui";

App({
  title: "Canvas Demo",
  width: 400,
  height: 320,
  body: VStack(0, [
    Canvas(400, 300, (ctx) => {
      // Background
      ctx.setFillColor("#1A1A2E");
      ctx.fillRect(0, 0, 400, 300);

      // Sun
      ctx.setFillColor("#FFD700");
      ctx.beginPath();
      ctx.arc(300, 80, 40, 0, Math.PI * 2);
      ctx.fill();

      // Ground
      ctx.setFillColor("#2D5016");
      ctx.fillRect(0, 220, 400, 80);

      // Tree trunk
      ctx.setFillColor("#8B4513");
      ctx.fillRect(80, 150, 20, 70);

      // Tree top
      ctx.setFillColor("#228B22");
      ctx.beginPath();
      ctx.arc(90, 130, 40, 0, Math.PI * 2);
      ctx.fill();
    }),
  ]),
});
```

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Animation](animation.md) — Animating widget properties
- [Styling](styling.md) — Widget styling
