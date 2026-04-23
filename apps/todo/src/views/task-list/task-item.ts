// ============================================================
// 任务行组件（Task Item Row）
//
// 单个任务行的结构：
//
//   [○/✓]  标题文字              [★/☆]  [📅 日期]
//    checkbox  title (flex)       star   due-chip
//
// 状态可见性规则：
//   - 星号（☆）：未重要时只在 hover 时显示，重要时（金色★）常驻
//   - 日期 chip：有截止日期时显示；逾期红色，今天蓝色，未来灰色
//   - 标题：已完成时变灰（Perry 无 strikethrough API，用颜色区分）
//   - 整行背景：selected = bgCard；hover = bgHover；normal = 透明
//
// 外部调用方式：
//   const item = buildTaskItem(task, { onSelect, onDoneToggle, onImportantToggle });
//   widgetAddChild(listContainer, item);
// ============================================================

import {
  HStack,
  Text,
  Spacer,
  widgetSetWidth,
  widgetSetHeight,
  widgetMatchParentWidth,
  widgetSetBackgroundColor,
  widgetSetOnClick,
  widgetSetOnHover,
  widgetSetHidden,
  setCornerRadius,
  setPadding,
  textSetColor,
  textSetFontSize,
  textSetFontWeight,
} from 'perry/ui';

import { theme, rgba, COLOR_STAR, COLOR_DANGER, COLOR_ACCENT } from '../../theme/colors';
import { Task }                from '../../types/index';
import { formatDueDate, isOverdue, isToday } from '../../utils/date';

// ============================================================
// 外部选中状态追踪
// 模块级别保存当前高亮的行背景 Widget 句柄
// 切换选中时：还原旧行背景 → 高亮新行背景
// ============================================================
let _selectedRowBg: any = null;

/**
 * 将上一个选中行背景还原为 normal，并高亮新行
 */
export function setSelectedRow(newBg: any): void {
  const c = theme();
  if (_selectedRowBg && _selectedRowBg !== newBg) {
    widgetSetBackgroundColor(_selectedRowBg, ...rgba(c.bgBase));
  }
  _selectedRowBg = newBg;
  widgetSetBackgroundColor(newBg, ...rgba(c.bgCard));
}

/**
 * 清除选中状态（关闭详情面板时调用）
 */
export function clearSelectedRow(): void {
  const c = theme();
  if (_selectedRowBg) {
    widgetSetBackgroundColor(_selectedRowBg, ...rgba(c.bgBase));
    _selectedRowBg = null;
  }
}

// ============================================================
// 构建选项
// ============================================================

export interface TaskItemCallbacks {
  /** 点击整行（标题区域）→ 选中任务，打开详情 */
  onSelect:           () => void;
  /** 点击复选框圆圈 → 切换完成状态 */
  onDoneToggle:       () => void;
  /** 点击星号 → 切换重要状态 */
  onImportantToggle:  () => void;
}

// ============================================================
// 主构建函数
// ============================================================

/**
 * 构建一个任务行 Widget
 *
 * @param task      任务数据
 * @param callbacks 交互回调（由任务列表视图传入）
 * @param isSelected 是否处于选中状态（初始渲染时决定背景色）
 * @returns 行容器 Widget
 */
export function buildTaskItem(
  task: Task,
  callbacks: TaskItemCallbacks,
  isSelected: boolean = false
): any {
  const c = theme();

  // ── 复选框 ────────────────────────────────────────────────
  // "○" 未完成；"✓" 已完成（accent 色）
  const checkbox = Text(task.isDone ? '✓' : '○');
  textSetFontSize(checkbox, 18);
  textSetColor(
    checkbox,
    ...(task.isDone ? rgba(COLOR_ACCENT) : rgba(c.textSecondary))
  );
  widgetSetWidth(checkbox, 32);
  widgetSetHeight(checkbox, 32);

  // 点击复选框只触发完成切换，不打开详情
  widgetSetOnClick(checkbox, callbacks.onDoneToggle);

  // ── 标题 ──────────────────────────────────────────────────
  // 已完成：灰色 + 降低字重；未完成：正常颜色
  const title = Text(task.title);
  textSetFontSize(title, 14);
  textSetColor(
    title,
    ...(task.isDone ? rgba(c.textDisabled) : rgba(c.textPrimary))
  );
  if (task.isDone) {
    // Perry 暂无 strikethrough API，用颜色（灰）表示已完成
    textSetFontWeight(title, 14, 300); // 细字重强化"已完成"感
  }

  // ── 截止日期 chip ──────────────────────────────────────────
  const dueDateChip = buildDueDateChip(task.dueDate, task.isDone);

  // ── 星号（重要标记）──────────────────────────────────────
  // 重要时：金色实心星 ★（常驻显示）
  // 不重要时：空心星 ☆（只在 hover 时显示）
  const star = Text(task.isImportant ? '★' : '☆');
  textSetFontSize(star, 16);
  textSetColor(
    star,
    ...(task.isImportant ? rgba(COLOR_STAR) : rgba(c.textDisabled))
  );
  widgetSetWidth(star, 28);
  // 不重要时默认隐藏星号，hover 时显示（见 onHover 逻辑）
  widgetSetHidden(star, task.isImportant ? 0 : 1);

  widgetSetOnClick(star, callbacks.onImportantToggle);

  // ── 行容器（HStack：checkbox + title + [chip] + star）────
  const row = HStack(0, [checkbox, title, Spacer(), dueDateChip, star]);
  widgetMatchParentWidth(row);
  widgetSetHeight(row, 44);
  setPadding(row, 0, 8, 0, 8);
  setCornerRadius(row, 6);

  // 初始背景：选中 = bgCard，其他 = 透明（bgBase）
  widgetSetBackgroundColor(
    row,
    ...(isSelected ? rgba(c.bgCard) : rgba(c.bgBase))
  );
  if (isSelected) _selectedRowBg = row;

  // ── 交互：hover ───────────────────────────────────────────
  widgetSetOnHover(row, (isEnter: boolean) => {
    const nc = theme();
    // 选中状态不响应 hover 背景变更
    if (row !== _selectedRowBg) {
      widgetSetBackgroundColor(row, ...(isEnter ? rgba(nc.bgHover) : rgba(nc.bgBase)));
    }
    // 未标重要时：hover 显示星号，离开时隐藏
    if (!task.isImportant) {
      widgetSetHidden(star, isEnter ? 0 : 1);
    }
  });

  // ── 交互：点击整行（标题区域）→ 选中 ────────────────────
  // 注意：Perry 中子 widget 的 onClick 会先于父 widget 触发；
  // checkbox 和 star 的 onClick 会独立处理，不会冒泡到 row。
  widgetSetOnClick(row, () => {
    setSelectedRow(row);
    callbacks.onSelect();
  });

  return row;
}

// ============================================================
// 截止日期 chip
// ============================================================

function buildDueDateChip(dueDate: number | null, isDone: boolean): any {
  const c = theme();

  if (!dueDate) {
    // 无截止日期：返回空占位（不占宽度）
    const empty = Text('');
    widgetSetWidth(empty, 0);
    widgetSetHeight(empty, 0);
    widgetSetHidden(empty, 1);
    return empty;
  }

  const label  = formatDueDate(dueDate);
  const over   = isOverdue(dueDate);
  const today  = isToday(dueDate);
  const chip   = Text(`📅 ${label}`);

  textSetFontSize(chip, 11);
  setPadding(chip, 2, 6, 2, 6);
  setCornerRadius(chip, 4);

  if (isDone) {
    // 已完成任务的日期 chip 用灰色
    textSetColor(chip, ...rgba(c.textDisabled));
    widgetSetBackgroundColor(chip, ...rgba(c.bgHover));
  } else if (over) {
    // 逾期：红色底 + 白/深色文字
    textSetColor(chip, ...rgba(COLOR_DANGER));
    widgetSetBackgroundColor(chip, ...rgba({ ...COLOR_DANGER, a: 0.12 }));
  } else if (today) {
    // 今天：accent 蓝色
    textSetColor(chip, ...rgba(COLOR_ACCENT));
    widgetSetBackgroundColor(chip, ...rgba({ ...COLOR_ACCENT, a: 0.12 }));
  } else {
    // 未来日期：次要文字色
    textSetColor(chip, ...rgba(c.textSecondary));
    widgetSetBackgroundColor(chip, ...rgba(c.bgHover));
  }

  return chip;
}
