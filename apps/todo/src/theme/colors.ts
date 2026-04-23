// ============================================================
// Fluent Design 色彩系统
//
// 参考 Microsoft Fluent Design System 和 WinUI 3 色板。
// 所有颜色使用 RGBA 浮点分量 (0.0-1.0)，直接匹配 Perry UI API：
//   widgetSetBackgroundColor(w, r, g, b, a)
//   textSetColor(w, r, g, b, a)
//   ...
// ============================================================

import { appState } from '../state/app-state';

// ============================================================
// 颜色类型
// ============================================================

/** RGBA 颜色（分量范围 0.0 - 1.0） */
export interface Color {
  r: number;
  g: number;
  b: number;
  a: number;
}

/** 将十六进制颜色字符串转换为 Color 对象（便于文档中阅读） */
export function hex(hexStr: string, alpha: number = 1.0): Color {
  const h = hexStr.replace('#', '');
  return {
    r: parseInt(h.slice(0, 2), 16) / 255,
    g: parseInt(h.slice(2, 4), 16) / 255,
    b: parseInt(h.slice(4, 6), 16) / 255,
    a: alpha,
  };
}

// ============================================================
// 固定语义色（亮暗模式共用）
// ============================================================

/** 强调色（Microsoft Blue） */
export const COLOR_ACCENT      = hex('#0078D4');
/** 强调色（较浅，hover/pressed 状态） */
export const COLOR_ACCENT_LIGHT = hex('#2899F5');
/** 强调色（较深，active 状态） */
export const COLOR_ACCENT_DARK  = hex('#005A9E');

/** 危险/删除色（红色） */
export const COLOR_DANGER   = hex('#E5392E');
/** 成功色（绿色） */
export const COLOR_SUCCESS  = hex('#3DBA92');
/** 警告色（橙色） */
export const COLOR_WARNING  = hex('#CA5010');
/** 星标/重要色（金黄色） */
export const COLOR_STAR     = hex('#FFC600');

/** 完全透明 */
export const COLOR_CLEAR: Color = { r: 0, g: 0, b: 0, a: 0 };

// ============================================================
// 主题色板定义
// ============================================================

interface ThemePalette {
  // ── 背景层级 ────────────────────────────────────────────
  /** 应用主背景（最外层） */
  bgBase: Color;
  /** 次级背景（卡片、面板） */
  bgSurface: Color;
  /** 侧边栏背景 */
  bgSidebar: Color;
  /** 任务卡片背景 */
  bgCard: Color;
  /** Hover 高亮背景 */
  bgHover: Color;
  /** 选中高亮背景 */
  bgSelected: Color;
  /** 快速输入框背景 */
  bgInput: Color;

  // ── 文字层级 ────────────────────────────────────────────
  /** 主文本（标题、任务名） */
  textPrimary: Color;
  /** 次级文本（副标题、日期、备注） */
  textSecondary: Color;
  /** 禁用/占位符文本 */
  textDisabled: Color;
  /** 强调文本（链接、计数器） */
  textAccent: Color;
  /** 已完成任务文本（删除线效果的颜色）*/
  textDone: Color;

  // ── 分割线 ──────────────────────────────────────────────
  divider: Color;
  dividerStrong: Color;

  // ── 按钮状态 ────────────────────────────────────────────
  btnPrimary: Color;
  btnPrimaryText: Color;
  btnSecondary: Color;
  btnSecondaryText: Color;

  // ── 侧边栏列表项 ─────────────────────────────────────────
  sidebarItemText: Color;
  sidebarItemSelected: Color;
  sidebarItemHover: Color;
}

// ============================================================
// 浅色主题（Light Mode）
// ============================================================

const LIGHT: ThemePalette = {
  bgBase:     hex('#F3F3F3'),  // Windows 11 Mica 效果近似色
  bgSurface:  hex('#FFFFFF'),
  bgSidebar:  hex('#EBEBEB'),
  bgCard:     hex('#FFFFFF'),
  bgHover:    hex('#E6E6E6'),
  bgSelected: hex('#CCE4F7'),  // 浅蓝选中
  bgInput:    hex('#FFFFFF'),

  textPrimary:   hex('#1A1A1A'),
  textSecondary: hex('#666666'),
  textDisabled:  hex('#ABABAB'),
  textAccent:    COLOR_ACCENT,
  textDone:      hex('#AAAAAA'),

  divider:       hex('#E0E0E0'),
  dividerStrong: hex('#C8C8C8'),

  btnPrimary:     COLOR_ACCENT,
  btnPrimaryText: hex('#FFFFFF'),
  btnSecondary:   hex('#E8E8E8'),
  btnSecondaryText: hex('#1A1A1A'),

  sidebarItemText:     hex('#2D2D2D'),
  sidebarItemSelected: hex('#CCE4F7'),
  sidebarItemHover:    hex('#DCDCDC'),
};

// ============================================================
// 深色主题（Dark Mode）
// ============================================================

const DARK: ThemePalette = {
  bgBase:     hex('#202020'),  // WinUI 3 深色基础色
  bgSurface:  hex('#2C2C2C'),
  bgSidebar:  hex('#262626'),
  bgCard:     hex('#303030'),
  bgHover:    hex('#3A3A3A'),
  bgSelected: hex('#003D6B'),  // 深蓝选中
  bgInput:    hex('#383838'),

  textPrimary:   hex('#F2F2F2'),
  textSecondary: hex('#A0A0A0'),
  textDisabled:  hex('#606060'),
  textAccent:    hex('#60ABEE'),  // 深色模式下浅蓝更易读
  textDone:      hex('#606060'),

  divider:       hex('#3A3A3A'),
  dividerStrong: hex('#4A4A4A'),

  btnPrimary:     hex('#0078D4'),
  btnPrimaryText: hex('#FFFFFF'),
  btnSecondary:   hex('#3C3C3C'),
  btnSecondaryText: hex('#F2F2F2'),

  sidebarItemText:     hex('#DDDDDD'),
  sidebarItemSelected: hex('#003D6B'),
  sidebarItemHover:    hex('#3A3A3A'),
};

// ============================================================
// 主题访问接口
// ============================================================

/**
 * 获取当前主题色板（根据 appState.isDark 自动切换）
 *
 * @example
 * const c = theme();
 * widgetSetBackgroundColor(card, c.bgCard.r, c.bgCard.g, c.bgCard.b, c.bgCard.a);
 */
export function theme(): ThemePalette {
  return appState.isDark ? DARK : LIGHT;
}

/**
 * 快捷展开 Color 为 Perry UI 函数参数
 * 减少调用处的样板代码
 *
 * @example
 * // 原始写法
 * widgetSetBackgroundColor(w, c.bgCard.r, c.bgCard.g, c.bgCard.b, c.bgCard.a);
 * // 使用辅助函数
 * widgetSetBackgroundColor(w, ...rgba(c.bgCard));
 */
export function rgba(c: Color): [number, number, number, number] {
  return [c.r, c.g, c.b, c.a];
}

// ============================================================
// 自定义列表色板（用户可为列表选择主题色）
// ============================================================

/** 预设的列表主题色（在列表创建/编辑对话框中展示） */
export const LIST_COLOR_PRESETS: Array<{ label: string; color: Color }> = [
  { label: '蓝色',   color: hex('#0078D4') },
  { label: '天空蓝', color: hex('#00BCF2') },
  { label: '绿色',   color: hex('#498205') },
  { label: '青绿',   color: hex('#00B294') },
  { label: '紫色',   color: hex('#8764B8') },
  { label: '粉紫',   color: hex('#C239B3') },
  { label: '红色',   color: hex('#E81123') },
  { label: '橙色',   color: hex('#CA5010') },
  { label: '金色',   color: hex('#986F0B') },
  { label: '灰色',   color: hex('#69797E') },
];
