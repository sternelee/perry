# Animation

Perry supports animating widget properties for smooth transitions.

## Opacity Animation

```typescript
import { Text } from "perry/ui";

const label = Text("Fading text");

// Animate from the widget's current opacity to `target` over `durationSecs`.
label.animateOpacity(1.0, 0.3); // target, durationSeconds
```

## Position Animation

```typescript
import { Button } from "perry/ui";

const btn = Button("Moving", () => {});

// Animate by a delta (dx, dy) relative to the widget's current position.
btn.animatePosition(100, 200, 0.5); // dx, dy, durationSeconds
```

## Example: Fade-In Effect

When the first argument reads from a `State.value`, Perry auto-subscribes
the call to the state — toggling `visible` re-runs the animation.

```typescript
import { App, Text, Button, VStack, State } from "perry/ui";

const visible = State(false);

const label = Text("Hello!");
label.animateOpacity(visible.value ? 1.0 : 0.0, 0.3);

App({
  title: "Animation Demo",
  width: 400,
  height: 300,
  body: VStack(16, [
    Button("Toggle", () => {
      visible.set(!visible.value);
    }),
    label,
  ]),
});
```

## Platform Notes

| Platform | Implementation |
|----------|---------------|
| macOS | NSAnimationContext / ViewPropertyAnimator |
| iOS | UIView.animate |
| Android | ViewPropertyAnimator |
| Windows | WM_TIMER-based animation |
| Linux | CSS transitions (GTK4) |
| Web | CSS transitions |

## Next Steps

- [Styling](styling.md) — Widget styling properties
- [Widgets](widgets.md) — All available widgets
- [Events](events.md) — User interaction
