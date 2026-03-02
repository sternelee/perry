// index.ts — perry-styling runtime library
//
// Provides:
//   - Re-exports from platform.ts (isMobile, isIOS, etc.)
//   - getTheme(theme)              — dark-mode resolver, call once at app startup
//   - applyTextColor(handle, r,g,b,a)    — text color
//   - applyFontSize(handle, size)         — font size (regular weight)
//   - applyFontBold(handle, size)         — font size with bold weight
//   - applyFontFamily(handle, family)     — custom font family
//   - applyBg(handle, r,g,b,a)           — widget background color
//   - applyRadius(handle, radius)         — widget corner radius
//   - applyWidth(handle, width)           — widget fixed width
//   - applyTooltip(handle, text)          — widget tooltip (no-op on iOS/Android)
//   - applyGradient(handle, r1,g1,b1,a1, r2,g2,b2,a2, direction) — background gradient
//   - applyButtonBg(handle, r,g,b,a)     — button background color
//   - applyButtonTextColor(handle, r,g,b,a) — button text color
//   - applyButtonBordered(handle, bordered) — button border flag
//
// All color parameters are f64 channels in [0, 1].
// Handle = i64 widget handle (NaN-boxed with POINTER_TAG in JS, passed as number).
//
// NOTE ON API DESIGN:
// Perry's compiler does not yet support passing arbitrary JS objects (e.g. PerryColor)
// as parameters to user-defined functions and then accessing their properties inside.
// The API therefore uses flat primitive parameters (r, g, b, a as separate numbers).
// At the call site, extract properties from a PerryColor before calling:
//
//   const c = t.colors.primary;
//   applyTextColor(handle, c.r, c.g, c.b, c.a);

export { PerryColor } from "./color";
export { Platform, isMac, isIOS, isAndroid, isWindows, isLinux, isDesktop, isMobile } from "./platform";

// -------------------------------------------------------------------
// Theme types (also re-exported from generated theme.ts)
// -------------------------------------------------------------------

import { PerryColor } from "./color";

export interface PerryColors { [key: string]: PerryColor }

export interface PerryTheme {
  light:       PerryColors;
  dark:        PerryColors;
  spacing:     { [key: string]: number };
  radius:      { [key: string]: number };
  fontSize:    { [key: string]: number };
  borderWidth: { [key: string]: number };
}

export interface ResolvedTheme {
  colors:      PerryColors;
  spacing:     { [key: string]: number };
  radius:      { [key: string]: number };
  fontSize:    { [key: string]: number };
  borderWidth: { [key: string]: number };
}

// -------------------------------------------------------------------
// Perry system built-ins
// -------------------------------------------------------------------

// isDarkMode() is a perry/system built-in — returns 1 if the OS is in dark mode.
import { isDarkMode } from "perry/system";

// -------------------------------------------------------------------
// Perry UI FFI function declarations
// -------------------------------------------------------------------

declare function perry_ui_widget_set_background_color(handle: number, r: number, g: number, b: number, a: number): void;
declare function perry_ui_widget_set_corner_radius(handle: number, radius: number): void;
declare function perry_ui_widget_set_width(handle: number, width: number): void;
declare function perry_ui_widget_set_tooltip(handle: number, text: string): void;
declare function perry_ui_widget_set_background_gradient(handle: number, r1: number, g1: number, b1: number, a1: number, r2: number, g2: number, b2: number, a2: number, direction: number): void;
declare function perry_ui_widget_set_border_color(handle: number, r: number, g: number, b: number, a: number): void;
declare function perry_ui_widget_set_border_width(handle: number, width: number): void;
declare function perry_ui_widget_set_edge_insets(handle: number, top: number, left: number, bottom: number, right: number): void;
declare function perry_ui_widget_set_opacity(handle: number, alpha: number): void;

declare function perry_ui_text_set_color(handle: number, r: number, g: number, b: number, a: number): void;
declare function perry_ui_text_set_font_size(handle: number, size: number): void;
declare function perry_ui_text_set_font_weight(handle: number, size: number, weight: number): void;
declare function perry_ui_text_set_font_family(handle: number, family: string): void;

declare function perry_ui_button_set_text_color(handle: number, r: number, g: number, b: number, a: number): void;
declare function perry_ui_button_set_bordered(handle: number, bordered: number): void;

// -------------------------------------------------------------------
// getTheme — resolve dark/light palette at app startup
// -------------------------------------------------------------------

export function getTheme(theme: PerryTheme): ResolvedTheme {
  const dark = isDarkMode();
  const colors = dark ? theme.dark : theme.light;
  return {
    colors,
    spacing:     theme.spacing,
    radius:      theme.radius,
    fontSize:    theme.fontSize,
    borderWidth: theme.borderWidth,
  };
}

// -------------------------------------------------------------------
// Text styling — flat primitive parameters (no PerryColor objects)
// -------------------------------------------------------------------

export function applyTextColor(handle: number, r: number, g: number, b: number, a: number): void {
  perry_ui_text_set_color(handle, r, g, b, a);
}

export function applyFontSize(handle: number, size: number): void {
  perry_ui_text_set_font_size(handle, size);
}

// Bold text — weight 1.0 = NSFontWeightBold
export function applyFontBold(handle: number, size: number): void {
  perry_ui_text_set_font_weight(handle, size, 1.0);
}

export function applyFontFamily(handle: number, family: string): void {
  perry_ui_text_set_font_family(handle, family);
}

// -------------------------------------------------------------------
// Widget styling — background, radius, width, tooltip
// -------------------------------------------------------------------

export function applyBg(handle: number, r: number, g: number, b: number, a: number): void {
  perry_ui_widget_set_background_color(handle, r, g, b, a);
}

export function applyRadius(handle: number, radius: number): void {
  perry_ui_widget_set_corner_radius(handle, radius);
}

export function applyWidth(handle: number, width: number): void {
  perry_ui_widget_set_width(handle, width);
}

export function applyTooltip(handle: number, text: string): void {
  perry_ui_widget_set_tooltip(handle, text);
}

// -------------------------------------------------------------------
// Gradient — direction: 0 = vertical (top→bottom), 1 = horizontal (left→right)
// -------------------------------------------------------------------

export function applyGradient(
  handle: number,
  r1: number, g1: number, b1: number, a1: number,
  r2: number, g2: number, b2: number, a2: number,
  direction: number,
): void {
  perry_ui_widget_set_background_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

// -------------------------------------------------------------------
// Button styling
// -------------------------------------------------------------------

export function applyButtonBg(handle: number, r: number, g: number, b: number, a: number): void {
  perry_ui_widget_set_background_color(handle, r, g, b, a);
}

export function applyButtonTextColor(handle: number, r: number, g: number, b: number, a: number): void {
  perry_ui_button_set_text_color(handle, r, g, b, a);
}

export function applyButtonBordered(handle: number, bordered: boolean): void {
  perry_ui_button_set_bordered(handle, bordered ? 1.0 : 0.0);
}

// -------------------------------------------------------------------
// Border, edge insets, opacity
// -------------------------------------------------------------------

export function applyBorderColor(handle: number, r: number, g: number, b: number, a: number): void {
  perry_ui_widget_set_border_color(handle, r, g, b, a);
}

export function applyBorderWidth(handle: number, width: number): void {
  perry_ui_widget_set_border_width(handle, width);
}

// Sets internal padding on VStack / HStack widgets (NSStackView edgeInsets).
// No-op for non-stack widgets.
export function applyEdgeInsets(handle: number, top: number, left: number, bottom: number, right: number): void {
  perry_ui_widget_set_edge_insets(handle, top, left, bottom, right);
}

export function applyOpacity(handle: number, alpha: number): void {
  perry_ui_widget_set_opacity(handle, alpha);
}
