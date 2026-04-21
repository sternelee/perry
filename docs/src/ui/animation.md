# Animation

Perry supports animating widget properties for smooth transitions.

## Opacity Animation

```typescript,no-test
import { Text } from "perry/ui";

const label = Text("Fading text");

// Animate from the widget's current opacity to `target` over `durationSecs`.
label.animateOpacity(1.0, 0.3); // target, durationSeconds
```

## Position Animation

```typescript,no-test
import { Button } from "perry/ui";

const btn = Button("Moving", () => {});

// Animate by a delta (dx, dy) relative to the widget's current position.
btn.animatePosition(100, 200, 0.5); // dx, dy, durationSeconds
```

## Example: Fade-In Effect

When the first argument reads from a `State.value`, Perry auto-subscribes
the call to the state — toggling `visible` re-runs the animation.

```typescript
{{#include ../../examples/ui/animation/fade_in.ts}}
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
