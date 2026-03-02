// color.ts — CSS color string → PerryColor (r,g,b,a floats in [0.0, 1.0])

export interface PerryColor {
  r: number;
  g: number;
  b: number;
  a: number;
}

// Parse two hex characters into a byte value (0–255)
function hexByte(s: string, offset: number): number {
  const hi = s.charAt(offset);
  const lo = s.charAt(offset + 1);
  return parseInt(hi + lo, 16);
}

// Parse one hex character into a nibble, then double it (e.g. "f" → 0xff)
function hexNibble(s: string, offset: number): number {
  const c = s.charAt(offset);
  return parseInt(c + c, 16);
}

// Convert a CSS color string to a PerryColor.
// Supports: #RGB, #RRGGBB, #RRGGBBAA, rgb(), rgba(), hsl(), hsla(), CSS named colors.
// Returns { r:1, g:0, b:1, a:1 } (magenta) as a visible error sentinel.
export function parseColor(css: string): PerryColor {
  const s = css.trim().toLowerCase();

  // --- Hex formats ---
  if (s.charAt(0) === "#") {
    const hex = s.slice(1);
    const len = hex.length;
    if (len === 3) {
      // #RGB
      const r = hexNibble(hex, 0) / 255.0;
      const g = hexNibble(hex, 1) / 255.0;
      const b = hexNibble(hex, 2) / 255.0;
      return { r, g, b, a: 1.0 };
    }
    if (len === 6) {
      // #RRGGBB
      const r = hexByte(hex, 0) / 255.0;
      const g = hexByte(hex, 2) / 255.0;
      const b = hexByte(hex, 4) / 255.0;
      return { r, g, b, a: 1.0 };
    }
    if (len === 8) {
      // #RRGGBBAA
      const r = hexByte(hex, 0) / 255.0;
      const g = hexByte(hex, 2) / 255.0;
      const b = hexByte(hex, 4) / 255.0;
      const a = hexByte(hex, 6) / 255.0;
      return { r, g, b, a };
    }
    if (len === 4) {
      // #RGBA (4-char shorthand)
      const r = hexNibble(hex, 0) / 255.0;
      const g = hexNibble(hex, 1) / 255.0;
      const b = hexNibble(hex, 2) / 255.0;
      const a = hexNibble(hex, 3) / 255.0;
      return { r, g, b, a };
    }
  }

  // --- rgb() / rgba() ---
  if (s.startsWith("rgb")) {
    // Strip "rgb(" or "rgba(" and ")"
    let inner = s;
    if (s.startsWith("rgba(")) {
      inner = s.slice(5);
    } else if (s.startsWith("rgb(")) {
      inner = s.slice(4);
    }
    // Remove trailing ")"
    const closeIdx = inner.indexOf(")");
    if (closeIdx >= 0) {
      inner = inner.slice(0, closeIdx);
    }
    const parts = inner.split(",");
    if (parts.length >= 3) {
      const r = parseFloat(parts[0].trim()) / 255.0;
      const g = parseFloat(parts[1].trim()) / 255.0;
      const b = parseFloat(parts[2].trim()) / 255.0;
      const a = parts.length >= 4 ? parseFloat(parts[3].trim()) : 1.0;
      return { r, g, b, a };
    }
  }

  // --- hsl() / hsla() ---
  if (s.startsWith("hsl")) {
    let inner = s;
    if (s.startsWith("hsla(")) {
      inner = s.slice(5);
    } else if (s.startsWith("hsl(")) {
      inner = s.slice(4);
    }
    const closeIdx = inner.indexOf(")");
    if (closeIdx >= 0) {
      inner = inner.slice(0, closeIdx);
    }
    const parts = inner.split(",");
    if (parts.length >= 3) {
      const h = parseFloat(parts[0].trim()) / 360.0;
      // Remove "%" and parse saturation/lightness
      let sPart = parts[1].trim();
      let lPart = parts[2].trim();
      if (sPart.endsWith("%")) {
        sPart = sPart.slice(0, sPart.length - 1);
      }
      if (lPart.endsWith("%")) {
        lPart = lPart.slice(0, lPart.length - 1);
      }
      const sat = parseFloat(sPart) / 100.0;
      const lig = parseFloat(lPart) / 100.0;
      const alpha = parts.length >= 4 ? parseFloat(parts[3].trim()) : 1.0;
      const rgb = hslToRgb(h, sat, lig);
      return { r: rgb[0], g: rgb[1], b: rgb[2], a: alpha };
    }
  }

  // --- CSS named colors (common subset) ---
  const named = namedColor(s);
  if (named !== null) {
    return named;
  }

  // Unknown / parse error — return magenta as a visible sentinel
  return { r: 1.0, g: 0.0, b: 1.0, a: 1.0 };
}

// HSL to RGB conversion. h, s, l in [0,1]. Returns [r, g, b] in [0,1].
function hslToRgb(h: number, s: number, l: number): number[] {
  if (s === 0.0) {
    return [l, l, l];
  }
  const q = l < 0.5 ? l * (1.0 + s) : l + s - l * s;
  const p = 2.0 * l - q;
  const r = hueToRgb(p, q, h + 1.0 / 3.0);
  const g = hueToRgb(p, q, h);
  const b = hueToRgb(p, q, h - 1.0 / 3.0);
  return [r, g, b];
}

function hueToRgb(p: number, q: number, t: number): number {
  let tt = t;
  if (tt < 0.0) { tt = tt + 1.0; }
  if (tt > 1.0) { tt = tt - 1.0; }
  if (tt < 1.0 / 6.0) { return p + (q - p) * 6.0 * tt; }
  if (tt < 1.0 / 2.0) { return q; }
  if (tt < 2.0 / 3.0) { return p + (q - p) * (2.0 / 3.0 - tt) * 6.0; }
  return p;
}

// A small table of CSS named colors (common ones relevant to design tokens)
function namedColor(name: string): PerryColor | null {
  if (name === "transparent") { return { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }; }
  if (name === "black")       { return { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }; }
  if (name === "white")       { return { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }; }
  if (name === "red")         { return { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }; }
  if (name === "green")       { return { r: 0.0, g: 0.502, b: 0.0, a: 1.0 }; }
  if (name === "blue")        { return { r: 0.0, g: 0.0, b: 1.0, a: 1.0 }; }
  if (name === "yellow")      { return { r: 1.0, g: 1.0, b: 0.0, a: 1.0 }; }
  if (name === "orange")      { return { r: 1.0, g: 0.647, b: 0.0, a: 1.0 }; }
  if (name === "purple")      { return { r: 0.502, g: 0.0, b: 0.502, a: 1.0 }; }
  if (name === "pink")        { return { r: 1.0, g: 0.753, b: 0.796, a: 1.0 }; }
  if (name === "gray" || name === "grey") { return { r: 0.502, g: 0.502, b: 0.502, a: 1.0 }; }
  if (name === "lightgray" || name === "lightgrey") { return { r: 0.827, g: 0.827, b: 0.827, a: 1.0 }; }
  if (name === "darkgray" || name === "darkgrey")   { return { r: 0.663, g: 0.663, b: 0.663, a: 1.0 }; }
  if (name === "silver")      { return { r: 0.753, g: 0.753, b: 0.753, a: 1.0 }; }
  if (name === "navy")        { return { r: 0.0, g: 0.0, b: 0.502, a: 1.0 }; }
  if (name === "teal")        { return { r: 0.0, g: 0.502, b: 0.502, a: 1.0 }; }
  if (name === "cyan") { return { r: 0.0, g: 1.0, b: 1.0, a: 1.0 }; }
  if (name === "magenta") { return { r: 1.0, g: 0.0, b: 1.0, a: 1.0 }; }
  if (name === "lime") { return { r: 0.0, g: 1.0, b: 0.0, a: 1.0 }; }
  if (name === "maroon") { return { r: 0.502, g: 0.0, b: 0.0, a: 1.0 }; }
  if (name === "olive") { return { r: 0.502, g: 0.502, b: 0.0, a: 1.0 }; }
  if (name === "aqua") { return { r: 0.0, g: 1.0, b: 1.0, a: 1.0 }; }
  if (name === "fuchsia") { return { r: 1.0, g: 0.0, b: 1.0, a: 1.0 }; }
  if (name === "indigo") { return { r: 0.294, g: 0.0, b: 0.510, a: 1.0 }; }
  if (name === "violet") { return { r: 0.933, g: 0.510, b: 0.933, a: 1.0 }; }
  if (name === "coral") { return { r: 1.0, g: 0.498, b: 0.314, a: 1.0 }; }
  if (name === "salmon") { return { r: 0.980, g: 0.502, b: 0.447, a: 1.0 }; }
  if (name === "tomato") { return { r: 1.0, g: 0.388, b: 0.278, a: 1.0 }; }
  if (name === "gold") { return { r: 1.0, g: 0.843, b: 0.0, a: 1.0 }; }
  if (name === "khaki") { return { r: 0.941, g: 0.902, b: 0.549, a: 1.0 }; }
  if (name === "beige") { return { r: 0.961, g: 0.961, b: 0.863, a: 1.0 }; }
  if (name === "ivory") { return { r: 1.0, g: 1.0, b: 0.941, a: 1.0 }; }
  if (name === "lavender") { return { r: 0.902, g: 0.902, b: 0.980, a: 1.0 }; }
  if (name === "turquoise") { return { r: 0.251, g: 0.878, b: 0.816, a: 1.0 }; }
  if (name === "chocolate") { return { r: 0.824, g: 0.412, b: 0.118, a: 1.0 }; }
  if (name === "brown") { return { r: 0.647, g: 0.165, b: 0.165, a: 1.0 }; }
  return null;
}

// Format a PerryColor as a 3-decimal string like "{ r: 0.231, g: 0.510, b: 0.965, a: 1.000 }"
export function formatColor(c: PerryColor): string {
  return "{ r: " + c.r.toFixed(3) + ", g: " + c.g.toFixed(3) + ", b: " + c.b.toFixed(3) + ", a: " + c.a.toFixed(3) + " }";
}

// Format a PerryColor as a compact TS object literal with inline comment
export function colorLiteral(c: PerryColor, origHex: string): string {
  return "{ r: " + c.r.toFixed(3) + ", g: " + c.g.toFixed(3) + ", b: " + c.b.toFixed(3) + ", a: " + c.a.toFixed(3) + " }  // " + origHex;
}
