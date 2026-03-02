// showcase.ts — perry-styling feature showcase with live theme switcher
//
// Build:
//   perry compile examples/showcase.ts -o examples/dist/showcase

import {
  App,
  VStack, HStack,
  Text, Button,
  State, Spacer, Divider,
  ScrollView,
  scrollviewSetChild,
  widgetAddChild,
  VStackWithInsets,
} from "perry/ui";

import {
  getTheme,
  applyTextColor, applyFontSize, applyFontBold, applyFontFamily,
  applyBg, applyRadius, applyWidth,
  applyGradient,
  applyButtonBg, applyButtonTextColor, applyButtonBordered,
  applyBorderColor, applyBorderWidth, applyEdgeInsets, applyOpacity,
  isMac, isMobile, isDesktop,
} from "../src/index";

import { theme } from "./theme";

// ---------------------------------------------------------------------------
// Resolve palette at startup
// ---------------------------------------------------------------------------
const t = getTheme(theme);

// Pre-extract all theme colors as flat module-level constants
const cPrimary = t.colors.primary;
const cSurface = t.colors.surface;
const cCard    = t.colors.card;
const cText    = t.colors.text;
const cMuted   = t.colors.muted;
const cAccent  = t.colors.accent;
const cDanger  = t.colors.danger;

// ---------------------------------------------------------------------------
// Theme palette table — 4 built-in themes (primary + accent RGBA)
// Theme 0: Blue  + Emerald  (default from tokens)
// Theme 1: Green + Amber
// Theme 2: Violet + Pink
// Theme 3: Amber + Red
// ---------------------------------------------------------------------------
//  Theme 0 — Blue #3B82F6 / Emerald #10B981
const TH0_PR = cPrimary.r; const TH0_PG = cPrimary.g; const TH0_PB = cPrimary.b;
const TH0_AR = cAccent.r;  const TH0_AG = cAccent.g;  const TH0_AB = cAccent.b;

//  Theme 1 — Green #059669 / Amber #F59E0B
const TH1_PR = 0.020; const TH1_PG = 0.588; const TH1_PB = 0.412;
const TH1_AR = 0.961; const TH1_AG = 0.620; const TH1_AB = 0.043;

//  Theme 2 — Violet #7C3AED / Pink #EC4899
const TH2_PR = 0.486; const TH2_PG = 0.227; const TH2_PB = 0.929;
const TH2_AR = 0.925; const TH2_AG = 0.282; const TH2_AB = 0.600;

//  Theme 3 — Amber #D97706 / Red #DC2626
const TH3_PR = 0.851; const TH3_PG = 0.467; const TH3_PB = 0.024;
const TH3_AR = 0.863; const TH3_AG = 0.149; const TH3_AB = 0.149;

// ---------------------------------------------------------------------------
// SECTION 1 — Header
// ---------------------------------------------------------------------------
const appTitle = Text("perry-styling Showcase");
applyFontBold(appTitle, t.fontSize.xl);
applyTextColor(appTitle, cPrimary.r, cPrimary.g, cPrimary.b, cPrimary.a);

const appSub = Text("Design tokens + ergonomic styling helpers for Perry UI");
applyFontSize(appSub, t.fontSize.sm);
applyTextColor(appSub, cMuted.r, cMuted.g, cMuted.b, cMuted.a);

// ---------------------------------------------------------------------------
// SECTION 2 — Text Styles
// ---------------------------------------------------------------------------
const heroText = Text("Hero Title — 36pt Bold");
applyFontBold(heroText, t.fontSize.hero);
applyTextColor(heroText, cText.r, cText.g, cText.b, cText.a);

const xlText = Text("Extra Large — 28pt Bold");
applyFontBold(xlText, t.fontSize.xl);
applyTextColor(xlText, cPrimary.r, cPrimary.g, cPrimary.b, cPrimary.a);

const lgText = Text("Large — 20pt Regular");
applyFontSize(lgText, t.fontSize.lg);
applyTextColor(lgText, cText.r, cText.g, cText.b, cText.a);

const baseText = Text("Base — 16pt. The quick brown fox jumps over the lazy dog.");
applyFontSize(baseText, t.fontSize.base);
applyTextColor(baseText, cText.r, cText.g, cText.b, cText.a);

const mutedText = Text("Caption — 12pt muted. Secondary information lives here.");
applyFontSize(mutedText, t.fontSize.sm);
applyTextColor(mutedText, cMuted.r, cMuted.g, cMuted.b, cMuted.a);

const accentText = Text("Accent — success / confirmation messaging.");
applyFontBold(accentText, t.fontSize.base);
applyTextColor(accentText, cAccent.r, cAccent.g, cAccent.b, cAccent.a);

const dangerText = Text("Danger — error / destructive action warning.");
applyFontBold(dangerText, t.fontSize.base);
applyTextColor(dangerText, cDanger.r, cDanger.g, cDanger.b, cDanger.a);

const monoText = Text("Menlo — perry_ui_text_set_font_family(\"Menlo\")");
applyFontSize(monoText, t.fontSize.base);
applyTextColor(monoText, cText.r, cText.g, cText.b, cText.a);
applyFontFamily(monoText, "Menlo");

// ---------------------------------------------------------------------------
// SECTION 3 — Button Styles
// ---------------------------------------------------------------------------
const primaryBtn = Button("Primary", () => {});
applyButtonBg(primaryBtn, cPrimary.r, cPrimary.g, cPrimary.b, cPrimary.a);
applyButtonTextColor(primaryBtn, cSurface.r, cSurface.g, cSurface.b, cSurface.a);
applyButtonBordered(primaryBtn, false);

const outlineBtn = Button("Outlined", () => {});
applyButtonBordered(outlineBtn, true);

const accentBtn = Button("Accent", () => {});
applyButtonBg(accentBtn, cAccent.r, cAccent.g, cAccent.b, cAccent.a);
applyButtonTextColor(accentBtn, cSurface.r, cSurface.g, cSurface.b, cSurface.a);
applyButtonBordered(accentBtn, false);

const dangerBtn = Button("Danger", () => {});
applyButtonBg(dangerBtn, cDanger.r, cDanger.g, cDanger.b, cDanger.a);
applyButtonTextColor(dangerBtn, cSurface.r, cSurface.g, cSurface.b, cSurface.a);
applyButtonBordered(dangerBtn, false);

// ---------------------------------------------------------------------------
// SECTION 4 — Styled Card
// ---------------------------------------------------------------------------
const cardHeader = Text("Styled Card");
applyFontBold(cardHeader, t.fontSize.lg);
applyTextColor(cardHeader, cText.r, cText.g, cText.b, cText.a);

const cardBody = Text("applyBg() + applyRadius() set background and corner radius.");
applyFontSize(cardBody, t.fontSize.sm);
applyTextColor(cardBody, cMuted.r, cMuted.g, cMuted.b, cMuted.a);

const card = VStackWithInsets(t.spacing.sm, t.spacing.md, t.spacing.md, t.spacing.md, t.spacing.md);
widgetAddChild(card, cardHeader);
widgetAddChild(card, cardBody);
applyBg(card, cCard.r, cCard.g, cCard.b, cCard.a);
applyRadius(card, t.radius.lg);
applyWidth(card, 540.0);

// ---------------------------------------------------------------------------
// SECTION 5 — Gradient Panels
// ---------------------------------------------------------------------------
const gradTitle = Text("Gradient — primary → accent, vertical");
applyFontBold(gradTitle, t.fontSize.lg);
applyTextColor(gradTitle, cSurface.r, cSurface.g, cSurface.b, cSurface.a);

const gradPanel = VStackWithInsets(t.spacing.sm, t.spacing.md, t.spacing.md, t.spacing.md, t.spacing.md);
widgetAddChild(gradPanel, gradTitle);
applyGradient(gradPanel, cPrimary.r, cPrimary.g, cPrimary.b, cPrimary.a,
                          cAccent.r,  cAccent.g,  cAccent.b,  cAccent.a, 0);
applyRadius(gradPanel, t.radius.lg);
applyWidth(gradPanel, 540.0);

const hGradTitle = Text("Gradient — accent → primary, horizontal");
applyFontBold(hGradTitle, t.fontSize.base);
applyTextColor(hGradTitle, cSurface.r, cSurface.g, cSurface.b, cSurface.a);

const hGradPanel = VStackWithInsets(t.spacing.sm, t.spacing.md, t.spacing.md, t.spacing.md, t.spacing.md);
widgetAddChild(hGradPanel, hGradTitle);
applyGradient(hGradPanel, cAccent.r,  cAccent.g,  cAccent.b,  cAccent.a,
                           cPrimary.r, cPrimary.g, cPrimary.b, cPrimary.a, 1);
applyRadius(hGradPanel, t.radius.md);
applyWidth(hGradPanel, 540.0);

// ---------------------------------------------------------------------------
// SECTION 6 — Live counter
// ---------------------------------------------------------------------------
const count = State(0);

const countDisplay = Text(`${count.value}`);
applyFontBold(countDisplay, t.fontSize.hero);
applyTextColor(countDisplay, cText.r, cText.g, cText.b, cText.a);

const incBtn = Button("+", () => count.set(count.value + 1));
applyButtonBg(incBtn, cAccent.r, cAccent.g, cAccent.b, cAccent.a);
applyButtonTextColor(incBtn, cSurface.r, cSurface.g, cSurface.b, cSurface.a);
applyButtonBordered(incBtn, false);

const decBtn = Button("−", () => count.set(count.value - 1));
applyButtonBg(decBtn, cMuted.r, cMuted.g, cMuted.b, cMuted.a);
applyButtonTextColor(decBtn, cSurface.r, cSurface.g, cSurface.b, cSurface.a);
applyButtonBordered(decBtn, false);

const resetBtn = Button("Reset", () => count.set(0));
applyButtonBordered(resetBtn, true);

const counterLabel = Text("LIVE COUNTER");
applyFontSize(counterLabel, t.fontSize.sm);
applyTextColor(counterLabel, cMuted.r, cMuted.g, cMuted.b, cMuted.a);

const counterCard = VStackWithInsets(t.spacing.sm, t.spacing.md, t.spacing.md, t.spacing.md, t.spacing.md);
widgetAddChild(counterCard, counterLabel);
widgetAddChild(counterCard, countDisplay);
widgetAddChild(counterCard, HStack(t.spacing.sm, [incBtn, decBtn, resetBtn]));
applyBg(counterCard, cCard.r, cCard.g, cCard.b, cCard.a);
applyRadius(counterCard, t.radius.lg);
applyWidth(counterCard, 540.0);

// ---------------------------------------------------------------------------
// SECTION 7 — Borders, edge insets & opacity
// ---------------------------------------------------------------------------

// Card with a prominent border — border color tracks the primary palette
const borderCardHeader = Text("Bordered Card — applyBorderColor() + applyBorderWidth()");
applyFontBold(borderCardHeader, t.fontSize.base);
applyTextColor(borderCardHeader, cText.r, cText.g, cText.b, cText.a);

const borderCardBody = Text("Border and inner padding (edge insets) are independent from radius.");
applyFontSize(borderCardBody, t.fontSize.sm);
applyTextColor(borderCardBody, cMuted.r, cMuted.g, cMuted.b, cMuted.a);

const borderCard = VStackWithInsets(t.spacing.sm, t.spacing.md, t.spacing.md, t.spacing.md, t.spacing.md);
widgetAddChild(borderCard, borderCardHeader);
widgetAddChild(borderCard, borderCardBody);
applyBg(borderCard, cCard.r, cCard.g, cCard.b, cCard.a);
applyRadius(borderCard, t.radius.lg);
applyBorderColor(borderCard, cPrimary.r, cPrimary.g, cPrimary.b, cPrimary.a);
applyBorderWidth(borderCard, t.borderWidth.md);
applyWidth(borderCard, 540.0);

// Opacity demo row — three labels fading out
const op100 = Text("Opacity 100%");
applyFontSize(op100, t.fontSize.base);
applyTextColor(op100, cText.r, cText.g, cText.b, cText.a);

const op60 = Text("Opacity 60%");
applyFontSize(op60, t.fontSize.base);
applyTextColor(op60, cText.r, cText.g, cText.b, cText.a);
applyOpacity(op60, 0.6);

const op20 = Text("Opacity 20%");
applyFontSize(op20, t.fontSize.base);
applyTextColor(op20, cText.r, cText.g, cText.b, cText.a);
applyOpacity(op20, 0.2);

const opRow = VStackWithInsets(t.spacing.sm, t.spacing.md, t.spacing.md, t.spacing.md, t.spacing.md);
widgetAddChild(opRow, op100);
widgetAddChild(opRow, op60);
widgetAddChild(opRow, op20);
applyBg(opRow, cCard.r, cCard.g, cCard.b, cCard.a);
applyRadius(opRow, t.radius.lg);
applyWidth(opRow, 540.0);

// ---------------------------------------------------------------------------
// SECTION 8 — Platform constants
// ---------------------------------------------------------------------------
const platformName = isMac     ? "macOS"        :
                     isMobile  ? "iOS / Android" :
                                 "Windows / Linux";
const layoutKind   = isDesktop ? "desktop" : "mobile";

const platformLine = Text("Platform: " + platformName + "   |   Layout: " + layoutKind);
applyFontSize(platformLine, t.fontSize.sm);
applyTextColor(platformLine, cMuted.r, cMuted.g, cMuted.b, cMuted.a);

const platformNote = Text("These are compile-time constants — Cranelift eliminates the dead branches.");
applyFontSize(platformNote, t.fontSize.sm);
applyTextColor(platformNote, cMuted.r, cMuted.g, cMuted.b, cMuted.a);

// ---------------------------------------------------------------------------
// Live theme switcher
// Re-applies primary + accent colors across all themed widgets immediately.
// applyTheme() takes only a number — no object params — Perry-compatible.
// ---------------------------------------------------------------------------
function applyTheme(idx: number): void {
  // Select primary RGBA for the chosen theme
  const pr = idx === 0 ? TH0_PR : (idx === 1 ? TH1_PR : (idx === 2 ? TH2_PR : TH3_PR));
  const pg = idx === 0 ? TH0_PG : (idx === 1 ? TH1_PG : (idx === 2 ? TH2_PG : TH3_PG));
  const pb = idx === 0 ? TH0_PB : (idx === 1 ? TH1_PB : (idx === 2 ? TH2_PB : TH3_PB));

  // Select accent RGBA for the chosen theme
  const ar = idx === 0 ? TH0_AR : (idx === 1 ? TH1_AR : (idx === 2 ? TH2_AR : TH3_AR));
  const ag = idx === 0 ? TH0_AG : (idx === 1 ? TH1_AG : (idx === 2 ? TH2_AG : TH3_AG));
  const ab = idx === 0 ? TH0_AB : (idx === 1 ? TH1_AB : (idx === 2 ? TH2_AB : TH3_AB));

  // Text colors that track the primary / accent
  applyTextColor(appTitle,   pr, pg, pb, 1.0);
  applyTextColor(xlText,     pr, pg, pb, 1.0);
  applyTextColor(accentText, ar, ag, ab, 1.0);

  // Buttons
  applyButtonBg(primaryBtn, pr, pg, pb, 1.0);
  applyButtonBg(accentBtn,  ar, ag, ab, 1.0);
  applyButtonBg(incBtn,     ar, ag, ab, 1.0);

  // Gradient panels — replace existing gradient with new colors
  applyGradient(gradPanel,  pr, pg, pb, 1.0, ar, ag, ab, 1.0, 0);
  applyGradient(hGradPanel, ar, ag, ab, 1.0, pr, pg, pb, 1.0, 1);

  // Border card — border tracks primary color
  applyBorderColor(borderCard, pr, pg, pb, 1.0);
}

// Theme switcher buttons — each tinted with its own primary color
const swBlue = Button("Blue", () => { applyTheme(0); });
applyButtonBg(swBlue, TH0_PR, TH0_PG, TH0_PB, 1.0);
applyButtonTextColor(swBlue, 1.0, 1.0, 1.0, 1.0);
applyButtonBordered(swBlue, false);

const swGreen = Button("Green", () => { applyTheme(1); });
applyButtonBg(swGreen, TH1_PR, TH1_PG, TH1_PB, 1.0);
applyButtonTextColor(swGreen, 1.0, 1.0, 1.0, 1.0);
applyButtonBordered(swGreen, false);

const swViolet = Button("Violet", () => { applyTheme(2); });
applyButtonBg(swViolet, TH2_PR, TH2_PG, TH2_PB, 1.0);
applyButtonTextColor(swViolet, 1.0, 1.0, 1.0, 1.0);
applyButtonBordered(swViolet, false);

const swAmber = Button("Amber", () => { applyTheme(3); });
applyButtonBg(swAmber, TH3_PR, TH3_PG, TH3_PB, 1.0);
applyButtonTextColor(swAmber, 1.0, 1.0, 1.0, 1.0);
applyButtonBordered(swAmber, false);

const swLabel = Text("THEME");
applyFontSize(swLabel, t.fontSize.sm);
applyTextColor(swLabel, cMuted.r, cMuted.g, cMuted.b, cMuted.a);

// Helper: small section label in muted color
function sectionLabel(label: string): number {
  const lbl = Text(label);
  applyFontSize(lbl, t.fontSize.sm);
  applyTextColor(lbl, cMuted.r, cMuted.g, cMuted.b, cMuted.a);
  return lbl;
}

// ---------------------------------------------------------------------------
// Compose
// ---------------------------------------------------------------------------
const content = VStack(t.spacing.md, [
  appTitle,
  appSub,

  // Theme switcher — always visible at the top
  swLabel,
  HStack(t.spacing.sm, [swBlue, swGreen, swViolet, swAmber]),
  Divider(),

  sectionLabel("TEXT STYLES"),
  heroText, xlText, lgText, baseText, mutedText, accentText, dangerText, monoText,
  Divider(),

  sectionLabel("BUTTON STYLES"),
  HStack(t.spacing.sm, [primaryBtn, outlineBtn, accentBtn, dangerBtn]),
  Divider(),

  sectionLabel("STYLED CARD — applyBg() + applyRadius()"),
  card,
  Divider(),

  sectionLabel("GRADIENTS — applyGradient()"),
  gradPanel,
  hGradPanel,
  Divider(),

  sectionLabel("LIVE COUNTER — themed widgets"),
  counterCard,
  Divider(),

  sectionLabel("BORDERS, PADDING & OPACITY"),
  borderCard,
  opRow,
  Divider(),

  sectionLabel("COMPILE-TIME PLATFORM CONSTANTS"),
  platformLine,
  platformNote,

  Spacer(),
]);

const scroll = ScrollView();
scrollviewSetChild(scroll, content);

App({
  title:  "perry-styling Showcase",
  width:  620,
  height: 700,
  body:   scroll,
});
