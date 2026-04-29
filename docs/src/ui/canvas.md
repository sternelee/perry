# Canvas

The `Canvas` widget provides a 2D drawing surface for custom graphics.

> **Availability**: `Canvas` is wired in the **LLVM** codegen path (macOS, iOS,
> Linux, Android) and the **JS / web / wasm** codegen path. Closed via
> [#190](https://github.com/PerryTS/perry/issues/190). The snippets below are
> compile-link verified by the doc-tests harness against
> [`docs/examples/ui/canvas/snippets.ts`](https://github.com/PerryTS/perry/blob/main/docs/examples/ui/canvas/snippets.ts);
> see that file for the full standalone program.

The drawing API is **method-based** on the canvas handle (matching the FFI
shape — `perry_ui_canvas_set_fill_color(handle, r, g, b, a)` etc.). Colors
are RGBA floats in `[0.0, 1.0]`.

## Creating a Canvas

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:create}}
```

`Canvas(width, height)` creates a canvas widget; subsequent draw operations
are method calls on the returned handle.

## Drawing Shapes

### Rectangles

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:rectangles}}
```

### Lines

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:lines}}
```

### Circles and Arcs

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:arcs}}
```

### Text

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:text}}
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
