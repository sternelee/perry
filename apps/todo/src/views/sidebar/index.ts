// ============================================================
// 侧边栏主视图（Sidebar）
//
// 布局结构（从上到下）：
//
//   ┌─────────────────────────────┐
//   │  🔍 搜索占位（Step 6 实现）  │  ← 顶部搜索框区域
//   ├─────────────────────────────┤
//   │  ☀ 我的一天           3     │  ← 智能列表（4 项固定）
//   │  ★ 重要               1     │
//   │  📅 计划内             2     │
//   │  ≡  全部               8     │
//   ├─────────────────────────────┤
//   │  我的列表                   │  ← 分区标题
//   │  ● 任务               5     │  ← 自定义列表（动态）
//   │  ● 购物               0     │
//   │  + 新建列表                 │  ← 底部操作行
//   └─────────────────────────────┘
//
// 刷新策略：
//   - sidebar 注册到 registerRefresh('sidebar')
//   - 每次刷新：清空列表容器 → 重建所有列表项
//   - 避免频繁刷新：只在 selectList / createList / deleteList 时触发
// ============================================================

import {
  VStack,
  HStack,
  Text,
  TextField,
  Button,
  Spacer,
  Divider,
  ScrollView,
  widgetAddChild,
  widgetClearChildren,
  widgetSetBackgroundColor,
  widgetMatchParentWidth,
  widgetMatchParentHeight,
  widgetSetHeight,
  widgetSetWidth,
  widgetSetHidden,
  widgetSetOnClick,
  setCornerRadius,
  setPadding,
  textSetColor,
  textSetFontSize,
  textSetFontWeight,
  scrollviewSetChild,
  textfieldSetString,
  textfieldGetString,
  textfieldFocus,
  textfieldSetOnSubmit,
  textfieldSetBorderless,
  textfieldSetFontSize,
  buttonSetBordered,
  buttonSetTextColor,
} from 'perry/ui';

import { theme, rgba, COLOR_ACCENT, COLOR_STAR } from '../../theme/colors';
import { appState, registerRefresh }             from '../../state/app-state';
import { SMART_LIST_META, SmartListMeta }        from '../../types/index';
import { getListBadgeCounts, createList }        from '../../services/list-service';
import { buildSidebarItem, attachListContextMenu } from './item';

// ============================================================
// 内部状态：新建列表输入框
// ============================================================

/** 新建列表输入行的显示状态 */
let _isCreatingList = false;
/** 新建列表输入框 Widget 句柄（用于显示/隐藏控制） */
let _createInputRow: any = null;
/** 新建列表输入框 TextField 句柄（用于聚焦 + 取值） */
let _createInputField: any = null;

// ============================================================
// 主构建函数
// ============================================================

/**
 * 构建完整侧边栏，返回根 Widget。
 *
 * 内部包含两个可变容器：
 *   - smartListContainer：智能列表区（固定 4 项，但 badge 数字需刷新）
 *   - customListContainer：自定义列表区（动态增删）
 *
 * 两个容器都注册到 registerRefresh('sidebar')，统一刷新。
 */
export function buildSidebar(): any {
  const c = theme();

  // ── 顶部：应用标题 ────────────────────────────────────────
  const appTitle = Text('Perry Todo');
  textSetFontSize(appTitle, 18);
  textSetFontWeight(appTitle, 18, 700);
  textSetColor(appTitle, ...rgba(c.textPrimary));

  const titleBar = HStack(0, [appTitle]);
  widgetMatchParentWidth(titleBar);
  widgetSetHeight(titleBar, 52);
  setPadding(titleBar, 0, 16, 0, 16);

  // ── 智能列表容器（固定区，角标动态更新）────────────────────
  const smartListContainer = VStack(2, []);
  widgetMatchParentWidth(smartListContainer);
  setPadding(smartListContainer, 4, 8, 4, 8);

  // ── 自定义列表标题行 ────────────────────────────────────────
  const sectionLabel = Text('我的列表');
  textSetFontSize(sectionLabel, 11);
  textSetFontWeight(sectionLabel, 11, 600);
  textSetColor(sectionLabel, ...rgba(c.textSecondary));

  const sectionHeader = HStack(0, [sectionLabel]);
  widgetMatchParentWidth(sectionHeader);
  widgetSetHeight(sectionHeader, 28);
  setPadding(sectionHeader, 0, 20, 0, 16);

  // ── 自定义列表容器（动态区）────────────────────────────────
  const customListContainer = VStack(2, []);
  widgetMatchParentWidth(customListContainer);
  setPadding(customListContainer, 0, 8, 4, 8);

  // ── 新建列表输入行 ──────────────────────────────────────────
  _createInputRow = buildCreateListRow(customListContainer);
  widgetSetHidden(_createInputRow, 1); // 默认隐藏

  // ── 底部：新建列表按钮 ──────────────────────────────────────
  const newListBtn = buildNewListButton(_createInputRow);

  // ── 可滚动内容区（包裹列表项，避免长列表超出窗口）──────────
  const scrollContent = VStack(0, [
    smartListContainer,
    buildDivider(),
    sectionHeader,
    customListContainer,
    _createInputRow,
  ]);
  widgetMatchParentWidth(scrollContent);

  const scrollView = ScrollView();
  scrollviewSetChild(scrollView, scrollContent);
  widgetMatchParentWidth(scrollView);

  // ── 侧边栏根容器 ────────────────────────────────────────────
  const sidebar = VStack(0, [
    titleBar,
    buildDivider(),
    scrollView,
    buildDivider(),
    newListBtn,
  ]);
  widgetMatchParentWidth(sidebar);
  widgetMatchParentHeight(sidebar);
  widgetSetBackgroundColor(sidebar, ...rgba(c.bgSidebar));

  // ── 注册刷新回调 ────────────────────────────────────────────
  // 每次触发时：拉取最新 badge 计数 → 清空两个容器 → 重建列表项
  registerRefresh('sidebar', () => {
    const nc      = theme();
    const badges  = getListBadgeCounts();

    // 更新侧边栏背景（主题切换时）
    widgetSetBackgroundColor(sidebar, ...rgba(nc.bgSidebar));
    textSetColor(appTitle, ...rgba(nc.textPrimary));
    textSetColor(sectionLabel, ...rgba(nc.textSecondary));

    // 重建智能列表
    rebuildSmartLists(smartListContainer, badges);

    // 重建自定义列表
    rebuildCustomLists(customListContainer, badges);
  });

  // 首次渲染
  const badges = getListBadgeCounts();
  rebuildSmartLists(smartListContainer, badges);
  rebuildCustomLists(customListContainer, badges);

  return sidebar;
}

// ============================================================
// 智能列表区重建
// ============================================================

/**
 * 清空并重建智能列表区的所有列表项
 */
function rebuildSmartLists(
  container: any,
  badges: Record<string, number>
): void {
  widgetClearChildren(container);

  // 智能列表图标颜色映射
  const iconColors: Record<string, any> = {
    'smart:my-day':    { r: 0.0, g: 0.478, b: 0.831, a: 1.0 }, // 蓝色
    'smart:important': COLOR_STAR,                               // 金色
    'smart:planned':   { r: 0.2, g: 0.7,   b: 0.4,   a: 1.0 }, // 绿色
    'smart:all':       { r: 0.5, g: 0.5,   b: 0.5,   a: 1.0 }, // 灰色
  };

  for (const meta of SMART_LIST_META) {
    const { bg } = buildSidebarItem({
      listId:       meta.id,
      icon:         meta.iconFallback,
      label:        meta.label,
      badgeCount:   badges[meta.id] ?? 0,
      accentColor:  iconColors[meta.id],
    });
    widgetAddChild(container, bg);
  }
}

// ============================================================
// 自定义列表区重建
// ============================================================

/**
 * 清空并重建自定义列表区的所有列表项
 */
function rebuildCustomLists(
  container: any,
  badges: Record<string, number>
): void {
  widgetClearChildren(container);

  const lists = appState.lists;

  if (lists.length === 0) {
    // 空列表提示
    const emptyHint = Text('点击下方"新建列表"开始创建');
    textSetFontSize(emptyHint, 12);
    textSetColor(emptyHint, ...rgba(theme().textDisabled));
    setPadding(emptyHint, 4, 20, 4, 20);
    widgetAddChild(container, emptyHint);
    return;
  }

  for (const list of lists) {
    const accentColor = {
      r: list.colorR,
      g: list.colorG,
      b: list.colorB,
      a: list.colorA,
    };

    const { bg } = buildSidebarItem({
      listId:      list.id,
      icon:        '●',
      label:       list.name,
      badgeCount:  badges[list.id] ?? 0,
      accentColor,
    });

    // 挂载右键菜单（重命名 / 修改颜色 / 删除）
    attachListContextMenu(bg, list, () => {
      // 操作完成后重建侧边栏
      const newBadges = getListBadgeCounts();
      rebuildCustomLists(container, newBadges);
    });

    widgetAddChild(container, bg);
  }
}

// ============================================================
// 新建列表输入行
// ============================================================

/**
 * 构建"新建列表"的内联输入行
 * （点击"+ 新建列表"按钮后展开，Enter 确认创建，Esc 取消）
 */
function buildCreateListRow(customListContainer: any): any {
  const c = theme();

  // 列表色点（默认蓝色，创建后用户可修改）
  const colorDot = Text('●');
  textSetFontSize(colorDot, 16);
  textSetColor(colorDot, ...rgba(COLOR_ACCENT));
  widgetSetWidth(colorDot, 28);

  // 输入框
  _createInputField = TextField('输入列表名称…', (_v: string) => {});
  textfieldSetBorderless(_createInputField, 1);
  textfieldSetFontSize(_createInputField, 14);
  widgetMatchParentWidth(_createInputField);

  // 确认按钮
  const confirmBtn = Button('添加', () => confirmCreate(customListContainer));
  buttonSetBordered(confirmBtn, 0);
  buttonSetTextColor(confirmBtn, ...rgba(COLOR_ACCENT));
  textSetFontSize(confirmBtn, 13);

  // Enter 提交
  textfieldSetOnSubmit(_createInputField, () => confirmCreate(customListContainer));

  const row = HStack(8, [colorDot, _createInputField, confirmBtn]);
  widgetMatchParentWidth(row);
  widgetSetHeight(row, 40);
  setPadding(row, 0, 12, 0, 8);
  setCornerRadius(row, 6);
  widgetSetBackgroundColor(row, ...rgba(c.bgCard));

  return row;
}

/**
 * 确认创建列表（从输入框读取名称 → 调用服务层 → 刷新）
 */
function confirmCreate(customListContainer: any): void {
  const name = textfieldGetString(_createInputField).trim();
  if (!name) {
    cancelCreate();
    return;
  }

  // 调用服务层创建（同步写数据库 + 更新 appState.lists）
  createList(name);

  // 重置输入框
  textfieldSetString(_createInputField, '');
  _isCreatingList = false;
  widgetSetHidden(_createInputRow, 1);

  // 重建自定义列表区（createList 已触发 sidebar 刷新，此处为保险）
  const badges = getListBadgeCounts();
  rebuildCustomLists(customListContainer, badges);
}

/**
 * 取消创建（隐藏输入行，清空内容）
 */
function cancelCreate(): void {
  textfieldSetString(_createInputField, '');
  _isCreatingList = false;
  widgetSetHidden(_createInputRow, 1);
}

// ============================================================
// "新建列表"底部按钮
// ============================================================

function buildNewListButton(createInputRow: any): any {
  const c = theme();

  const plusIcon = Text('+');
  textSetFontSize(plusIcon, 18);
  textSetColor(plusIcon, ...rgba(c.textAccent));
  widgetSetWidth(plusIcon, 28);

  const btnLabel = Text('新建列表');
  textSetFontSize(btnLabel, 14);
  textSetColor(btnLabel, ...rgba(c.textAccent));

  const btn = HStack(8, [plusIcon, btnLabel]);
  widgetMatchParentWidth(btn);
  widgetSetHeight(btn, 44);
  setPadding(btn, 0, 12, 0, 12);

  widgetSetOnClick(btn, () => {
    if (_isCreatingList) {
      cancelCreate();
    } else {
      _isCreatingList = true;
      widgetSetHidden(createInputRow, 0);
      // Perry 的 TextField 聚焦需在 UI 事件循环后执行
      textfieldFocus(_createInputField);
    }
  });

  // Hover 效果
  widgetSetOnHover(btn, (isEnter: boolean) => {
    const nc = theme();
    widgetSetBackgroundColor(
      btn,
      ...(isEnter ? rgba(nc.bgHover) : rgba(nc.bgSidebar))
    );
  });

  widgetSetBackgroundColor(btn, ...rgba(c.bgSidebar));

  return btn;
}

// ============================================================
// 工具：细分割线
// ============================================================

function buildDivider(): any {
  const c    = theme();
  const line = Divider();
  widgetMatchParentWidth(line);
  widgetSetHeight(line, 1);
  widgetSetBackgroundColor(line, ...rgba(c.divider));
  return line;
}
