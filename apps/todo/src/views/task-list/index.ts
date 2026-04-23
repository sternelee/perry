// ============================================================
// 任务列表视图（Task List Panel）
//
// 布局（自上而下固定+滚动组合）：
//
//   ┌──────────────────────────────┐
//   │  ☀ 我的一天         [···]   │  ← Header（固定）
//   │  4月23日 星期三              │  ← 副标题（智能列表专用）
//   ├──────────────────────────────┤
//   │  ○  添加任务…               │  ← 快速输入框（固定）
//   ├──────────────────────────────┤
//   │  (滚动区)                    │
//   │  ○  买牛奶           ★ 📅   │  ← 未完成任务
//   │  ○  完成报告                  │
//   │  ──────────────────────────  │
//   │  ▼ 已完成 (2)               │  ← 折叠/展开 Header
//   │    ✓  发邮件                 │  ← 已完成任务（折叠时隐藏）
//   └──────────────────────────────┘
//
// 刷新策略（注册 key: 'task-list'）：
//   - 清空 activeContainer / completedContainer
//   - 按 isDone 分组重建任务行
//   - 更新 header 标题、副标题、已完成计数
// ============================================================

import {
  VStack,
  HStack,
  Text,
  TextField,
  Button,
  ScrollView,
  Divider,
  Spacer,
  widgetAddChild,
  widgetClearChildren,
  widgetSetBackgroundColor,
  widgetMatchParentWidth,
  widgetMatchParentHeight,
  widgetSetHeight,
  widgetSetHidden,
  widgetSetOnClick,
  widgetSetOnHover,
  setCornerRadius,
  setPadding,
  textSetColor,
  textSetFontSize,
  textSetFontWeight,
  textSetString,
  scrollviewSetChild,
  textfieldSetString,
  textfieldGetString,
  textfieldFocus,
  textfieldSetOnSubmit,
  textfieldSetBorderless,
  textfieldSetFontSize,
  textfieldSetPlaceholder,
  menuCreate,
  menuAddItem,
  menuAddSeparator,
  widgetSetContextMenu,
  buttonSetTextColor,
  buttonSetBordered,
} from 'perry/ui';

import { theme, rgba, COLOR_ACCENT, COLOR_STAR } from '../../theme/colors';
import {
  appState,
  selectTask,
  toggleCompletedExpanded,
  registerRefresh,
} from '../../state/app-state';
import {
  SMART_LIST,
  SMART_LIST_META,
  SmartListId,
  isSmartList,
} from '../../types/index';
import { getMyDaySubtitle } from '../../utils/date';
import {
  createTask,
  toggleTaskDone,
  toggleTaskImportant,
  clearCompletedTasks,
  loadCurrentTasks,
} from '../../services/task-service';
import {
  buildTaskItem,
  clearSelectedRow,
} from './task-item';

// ============================================================
// 主构建函数
// ============================================================

/**
 * 构建任务列表面板，返回根 Widget。
 *
 * 面板包含两个可变容器：
 *   - activeContainer：未完成任务区（动态重建）
 *   - completedContainer：已完成任务区（折叠控制 + 动态重建）
 *
 * 注册到 registerRefresh('task-list')。
 */
export function buildTaskListPanel(): any {
  const c = theme();

  // ── Header：标题行 ────────────────────────────────────────
  const titleText = Text(getHeaderTitle());
  textSetFontSize(titleText, 20);
  textSetFontWeight(titleText, 20, 700);
  textSetColor(titleText, ...rgba(c.textPrimary));

  // 副标题（My Day 显示日期，其他视图空）
  const subtitleText = Text(getHeaderSubtitle());
  textSetFontSize(subtitleText, 12);
  textSetColor(subtitleText, ...rgba(c.textSecondary));
  widgetSetHidden(subtitleText, hasSubtitle() ? 0 : 1);

  // 列表操作按钮（⋯ 溢出菜单）
  const moreBtn = Text('···');
  textSetFontSize(moreBtn, 20);
  textSetColor(moreBtn, ...rgba(c.textSecondary));
  widgetSetOnClick(moreBtn, () => {/* 触发 contextMenu */});

  // Header 容器
  const header = VStack(2, [
    HStack(0, [titleText, Spacer(), moreBtn]),
    subtitleText,
  ]);
  widgetMatchParentWidth(header);
  widgetSetHeight(header, hasSubtitle() ? 64 : 48);
  setPadding(header, 8, 16, 8, 16);
  widgetSetBackgroundColor(header, ...rgba(c.bgBase));

  // 挂载右键菜单到 moreBtn（也可点击触发）
  attachListOptionsMenu(moreBtn, titleText);

  // ── 快速输入框 ────────────────────────────────────────────
  const { quickAddRow, quickAddField } = buildQuickAddBar();

  // ── 活跃任务容器（动态区，未完成） ────────────────────────
  const activeContainer = VStack(0, []);
  widgetMatchParentWidth(activeContainer);
  setPadding(activeContainer, 4, 8, 4, 8);

  // ── 已完成区 Header（折叠/展开控制） ─────────────────────
  const completedHeaderLabel  = Text('▶ 已完成 (0)');
  const completedHeaderRow    = buildCompletedHeader(completedHeaderLabel);

  // ── 已完成任务容器 ────────────────────────────────────────
  const completedContainer = VStack(0, []);
  widgetMatchParentWidth(completedContainer);
  setPadding(completedContainer, 4, 8, 4, 8);
  widgetSetHidden(completedContainer, appState.isCompletedExpanded ? 0 : 1);

  // ── 空状态提示（无未完成任务时显示） ─────────────────────
  const emptyState = buildEmptyState();

  // ── 可滚动内容区 ──────────────────────────────────────────
  const scrollContent = VStack(0, [
    activeContainer,
    emptyState,
    buildDivider(),
    completedHeaderRow,
    completedContainer,
  ]);
  widgetMatchParentWidth(scrollContent);

  const scrollView = ScrollView();
  scrollviewSetChild(scrollView, scrollContent);
  widgetMatchParentWidth(scrollView);
  widgetMatchParentHeight(scrollView);

  // ── 面板根容器 ────────────────────────────────────────────
  const root = VStack(0, [
    header,
    buildDivider(),
    quickAddRow,
    buildDivider(),
    scrollView,
  ]);
  widgetMatchParentWidth(root);
  widgetMatchParentHeight(root);
  widgetSetBackgroundColor(root, ...rgba(c.bgBase));

  // ── 注册刷新回调 ──────────────────────────────────────────
  registerRefresh('task-list', () => {
    const nc      = theme();
    const active  = appState.tasks.filter(t => !t.isDone);
    const done    = appState.tasks.filter(t => t.isDone);

    // 更新 Header
    textSetString(titleText,    getHeaderTitle());
    textSetString(subtitleText, getHeaderSubtitle());
    textSetColor(titleText,    ...rgba(nc.textPrimary));
    textSetColor(subtitleText, ...rgba(nc.textSecondary));
    widgetSetHidden(subtitleText, hasSubtitle() ? 0 : 1);
    widgetSetBackgroundColor(header, ...rgba(nc.bgBase));
    widgetSetBackgroundColor(root,   ...rgba(nc.bgBase));

    // 重建未完成任务
    widgetClearChildren(activeContainer);
    for (const task of active) {
      const isSelected = appState.selectedTaskId === task.id;
      const row = buildTaskItem(task, {
        onSelect:          () => { selectTask(task.id); },
        onDoneToggle:      () => { toggleTaskDone(task.id); },
        onImportantToggle: () => { toggleTaskImportant(task.id); },
      }, isSelected);
      widgetAddChild(activeContainer, row);
    }

    // 空状态
    widgetSetHidden(emptyState, active.length > 0 ? 1 : 0);

    // 更新已完成区 Header
    const doneLabel = appState.isCompletedExpanded ? '▼' : '▶';
    textSetString(completedHeaderLabel,
      `${doneLabel} 已完成 (${done.length})`);
    widgetSetHidden(completedHeaderRow, done.length === 0 ? 1 : 0);

    // 重建已完成任务
    widgetClearChildren(completedContainer);
    if (appState.isCompletedExpanded) {
      for (const task of done) {
        const row = buildTaskItem(task, {
          onSelect:          () => { selectTask(task.id); },
          onDoneToggle:      () => { toggleTaskDone(task.id); },
          onImportantToggle: () => { toggleTaskImportant(task.id); },
        }, appState.selectedTaskId === task.id);
        widgetAddChild(completedContainer, row);
      }
    }
    widgetSetHidden(completedContainer,
      appState.isCompletedExpanded && done.length > 0 ? 0 : 1);

    // 若选中任务被删除，清除高亮
    if (appState.selectedTaskId === null) {
      clearSelectedRow();
    }
  });

  // 首次渲染（触发一次刷新）
  loadCurrentTasks();

  return root;
}

// ============================================================
// Header：标题计算
// ============================================================

function getHeaderTitle(): string {
  const id = appState.selectedListId;
  const meta = SMART_LIST_META.find(m => m.id === id);
  if (meta) return meta.label;
  const list = appState.lists.find(l => l.id === id);
  return list?.name ?? '任务';
}

function getHeaderSubtitle(): string {
  if (appState.selectedListId === SMART_LIST.MY_DAY) {
    return getMyDaySubtitle();
  }
  return '';
}

function hasSubtitle(): boolean {
  return appState.selectedListId === SMART_LIST.MY_DAY;
}

// ============================================================
// 快速输入框（常驻于列表顶部）
// ============================================================

function buildQuickAddBar(): { quickAddRow: any; quickAddField: any } {
  const c = theme();

  const plusIcon = Text('○');
  textSetFontSize(plusIcon, 18);
  textSetColor(plusIcon, ...rgba(c.textSecondary));

  const field = TextField('添加任务', (_v: string) => {});
  textfieldSetBorderless(field, 1);
  textfieldSetFontSize(field, 14);
  widgetMatchParentWidth(field);

  const submitTask = () => {
    const title = textfieldGetString(field).trim();
    if (!title) return;
    textfieldSetString(field, '');
    createTask(title);
    // createTask 内部已触发 task-list 刷新
  };

  textfieldSetOnSubmit(field, submitTask);

  const row = HStack(10, [plusIcon, field]);
  widgetMatchParentWidth(row);
  widgetSetHeight(row, 44);
  setPadding(row, 0, 12, 0, 16);
  widgetSetBackgroundColor(row, ...rgba(c.bgBase));

  // 点击行任意区域聚焦输入框
  widgetSetOnClick(row, () => {
    textfieldFocus(field);
  });

  return { quickAddRow: row, quickAddField: field };
}

// ============================================================
// 已完成区折叠 Header
// ============================================================

function buildCompletedHeader(labelText: any): any {
  const c = theme();

  // 清除已完成按钮
  const clearBtn = Text('清除');
  textSetFontSize(clearBtn, 12);
  textSetColor(clearBtn, ...rgba(c.textAccent));
  widgetSetOnClick(clearBtn, clearCompletedTasks);
  widgetSetOnHover(clearBtn, (enter: boolean) => {
    const nc = theme();
    textSetColor(clearBtn, ...rgba(enter ? nc.textPrimary : nc.textAccent));
  });

  const row = HStack(0, [labelText, Spacer(), clearBtn]);
  widgetMatchParentWidth(row);
  widgetSetHeight(row, 36);
  setPadding(row, 0, 16, 0, 12);
  widgetSetBackgroundColor(row, ...rgba(c.bgBase));

  // 点击整行切换折叠/展开
  widgetSetOnClick(row, toggleCompletedExpanded);

  // Hover 效果
  widgetSetOnHover(row, (enter: boolean) => {
    const nc = theme();
    widgetSetBackgroundColor(row, ...(enter ? rgba(nc.bgHover) : rgba(nc.bgBase)));
  });

  textSetFontSize(labelText, 13);
  textSetFontWeight(labelText, 13, 600);
  textSetColor(labelText, ...rgba(c.textSecondary));

  return row;
}

// ============================================================
// 空状态提示
// ============================================================

function buildEmptyState(): any {
  const c = theme();

  const icon = Text('✓');
  textSetFontSize(icon, 32);
  textSetColor(icon, ...rgba(c.textDisabled));

  const msg = Text('此列表中没有未完成的任务');
  textSetFontSize(msg, 13);
  textSetColor(msg, ...rgba(c.textDisabled));

  const hint = Text('使用上方输入框添加任务');
  textSetFontSize(hint, 12);
  textSetColor(hint, ...rgba({ ...c.textDisabled, a: 0.6 }));

  const box = VStack(8, [icon, msg, hint]);
  widgetMatchParentWidth(box);
  setPadding(box, 48, 16, 48, 16);

  // 默认显示（首次加载前无任务）
  return box;
}

// ============================================================
// 列表选项菜单（⋯ 按钮）
// ============================================================

function attachListOptionsMenu(moreBtn: any, titleWidget: any): void {
  const menu = menuCreate();

  menuAddItem(menu, '清除已完成任务', () => {
    clearCompletedTasks();
  });

  menuAddSeparator(menu);

  menuAddItem(menu, '排序：默认（创建时间）', () => {
    // Step 6 实现排序切换
  });
  menuAddItem(menu, '排序：按截止日期', () => {
    // Step 6 实现
  });
  menuAddItem(menu, '排序：按重要性', () => {
    // Step 6 实现
  });

  widgetSetContextMenu(moreBtn, menu);
}

// ============================================================
// 分割线工具
// ============================================================

function buildDivider(): any {
  const c = theme();
  const d = Divider();
  widgetMatchParentWidth(d);
  widgetSetHeight(d, 1);
  widgetSetBackgroundColor(d, ...rgba(c.divider));
  return d;
}
