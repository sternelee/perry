// ============================================================
// 移动端"列表"Tab 视图
//
// 功能：展示所有可选列表（智能列表 + 自定义列表），
//       用户点击后切换 selectedListId 并跳转到任务列表页（Tab 0 或推入 NavStack）
//
// 布局结构：
//   ┌─────────────────────┐
//   │  列表                │  ← 页面标题
//   ├─────────────────────┤
//   │  ☀ 我的一天     3   │
//   │  ★ 重要         1   │
//   │  📅 计划内       2   │
//   │  ≡  全部         8   │
//   ├─────────────────────┤
//   │  我的列表            │  ← 分区标题
//   │  ● 任务         5   │
//   │  ● 购物         0   │
//   ├─────────────────────┤
//   │  ＋ 新建列表         │  ← 底部操作行
//   └─────────────────────┘
//
// 注意：移动端不复用桌面侧边栏组件（buildSidebarItem），
//       因为交互模型不同：移动端点击后切换到其他 Tab 的任务列表视图，
//       而桌面端点击只更新同一窗口的中栏。
//       视觉样式仍保持一致（同色调、同字体大小）。
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
  widgetSetWidth,
  widgetSetHidden,
  widgetSetOnClick,
  widgetSetOnHover,
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
import { appState, selectList, registerRefresh }  from '../../state/app-state';
import { SMART_LIST_META }                        from '../../types/index';
import { getListBadgeCounts, createList }         from '../../services/list-service';
import { loadCurrentTasks }                       from '../../services/task-service';

// ============================================================
// 内部状态：新建列表输入框
// ============================================================
let _isCreating   = false;
let _inputRow: any  = null;
let _inputField: any = null;

// ============================================================
// 主构建函数
// ============================================================

/**
 * 构建移动端"列表"Tab 的内容视图
 *
 * @param onSelectList 点击列表项后的回调（通知 mobile layout 切换 Tab）
 */
export function buildMobileListsTab(onSelectList: (listId: string) => void): any {
  const c = theme();

  // ── 页面标题 ─────────────────────────────────────────────
  const pageTitle = Text('列表');
  textSetFontSize(pageTitle, 22);
  textSetFontWeight(pageTitle, 22, 700);
  textSetColor(pageTitle, ...rgba(c.textPrimary));

  const titleBar = HStack(0, [pageTitle]);
  widgetMatchParentWidth(titleBar);
  widgetSetHeight(titleBar, 52);
  setPadding(titleBar, 0, 16, 0, 16);

  // ── 智能列表容器（固定区）───────────────────────────────
  const smartContainer = VStack(4, []);
  widgetMatchParentWidth(smartContainer);
  setPadding(smartContainer, 4, 12, 4, 12);

  // ── 自定义列表分区标题 ───────────────────────────────────
  const sectionLabel = Text('我的列表');
  textSetFontSize(sectionLabel, 11);
  textSetFontWeight(sectionLabel, 11, 600);
  textSetColor(sectionLabel, ...rgba(c.textSecondary));

  const sectionHeader = HStack(0, [sectionLabel]);
  widgetMatchParentWidth(sectionHeader);
  widgetSetHeight(sectionHeader, 28);
  setPadding(sectionHeader, 0, 20, 0, 16);

  // ── 自定义列表容器（动态区）─────────────────────────────
  const customContainer = VStack(4, []);
  widgetMatchParentWidth(customContainer);
  setPadding(customContainer, 0, 12, 4, 12);

  // ── 新建列表输入行 ───────────────────────────────────────
  _inputRow = buildMobileCreateRow(customContainer, onSelectList);
  widgetSetHidden(_inputRow, 1);

  // ── 新建列表按钮（底部固定） ─────────────────────────────
  const newListBtn = buildMobileNewListButton(_inputRow);

  // ── 可滚动内容区 ──────────────────────────────────────────
  const scrollContent = VStack(0, [
    smartContainer,
    buildMobileDivider(),
    sectionHeader,
    customContainer,
    _inputRow,
  ]);
  widgetMatchParentWidth(scrollContent);

  const scrollView = ScrollView();
  scrollviewSetChild(scrollView, scrollContent);
  widgetMatchParentWidth(scrollView);
  widgetMatchParentHeight(scrollView);

  // ── 页面根容器 ────────────────────────────────────────────
  const root = VStack(0, [titleBar, buildMobileDivider(), scrollView, newListBtn]);
  widgetMatchParentWidth(root);
  widgetMatchParentHeight(root);
  widgetSetBackgroundColor(root, ...rgba(c.bgBase));

  // ── 注册刷新回调 ──────────────────────────────────────────
  registerRefresh('mobile-lists-tab', () => {
    const nc     = theme();
    const badges = getListBadgeCounts();

    widgetSetBackgroundColor(root, ...rgba(nc.bgBase));
    textSetColor(pageTitle, ...rgba(nc.textPrimary));
    textSetColor(sectionLabel, ...rgba(nc.textSecondary));

    rebuildMobileSmartItems(smartContainer, badges, onSelectList);
    rebuildMobileCustomItems(customContainer, badges, onSelectList);
  });

  // 首次渲染
  const badges = getListBadgeCounts();
  rebuildMobileSmartItems(smartContainer, badges, onSelectList);
  rebuildMobileCustomItems(customContainer, badges, onSelectList);

  return root;
}

// ============================================================
// 智能列表区重建
// ============================================================

const SMART_ICON_COLORS: Record<string, any> = {
  'smart:my-day':    { r: 0.0, g: 0.478, b: 0.831, a: 1.0 },
  'smart:important': COLOR_STAR,
  'smart:planned':   { r: 0.2, g: 0.7,   b: 0.4,   a: 1.0 },
  'smart:all':       { r: 0.5, g: 0.5,   b: 0.5,   a: 1.0 },
};

function rebuildMobileSmartItems(
  container: any,
  badges: Record<string, number>,
  onSelectList: (id: string) => void
): void {
  widgetClearChildren(container);
  for (const meta of SMART_LIST_META) {
    const row = buildMobileListRow({
      icon:        meta.iconFallback,
      label:       meta.label,
      badgeCount:  badges[meta.id] ?? 0,
      iconColor:   SMART_ICON_COLORS[meta.id],
      isSelected:  appState.selectedListId === meta.id,
      onTap() {
        selectList(meta.id);
        loadCurrentTasks();
        onSelectList(meta.id);
      },
    });
    widgetAddChild(container, row);
  }
}

// ============================================================
// 自定义列表区重建
// ============================================================

function rebuildMobileCustomItems(
  container: any,
  badges: Record<string, number>,
  onSelectList: (id: string) => void
): void {
  widgetClearChildren(container);

  const lists = appState.lists;
  if (lists.length === 0) {
    const hint = Text('点击下方"新建列表"开始创建');
    textSetFontSize(hint, 12);
    textSetColor(hint, ...rgba(theme().textDisabled));
    setPadding(hint, 4, 20, 4, 20);
    widgetAddChild(container, hint);
    return;
  }

  for (const list of lists) {
    const iconColor = { r: list.colorR, g: list.colorG, b: list.colorB, a: list.colorA };
    const row = buildMobileListRow({
      icon:       '●',
      label:      list.name,
      badgeCount: badges[list.id] ?? 0,
      iconColor,
      isSelected: appState.selectedListId === list.id,
      onTap() {
        selectList(list.id);
        loadCurrentTasks();
        onSelectList(list.id);
      },
    });
    widgetAddChild(container, row);
  }
}

// ============================================================
// 单行列表项
// ============================================================

interface MobileListRowOptions {
  icon:       string;
  label:      string;
  badgeCount: number;
  iconColor:  any;
  isSelected: boolean;
  onTap:      () => void;
}

function buildMobileListRow(opts: MobileListRowOptions): any {
  const c = theme();

  // 图标
  const icon = Text(opts.icon);
  textSetFontSize(icon, 18);
  textSetColor(icon, ...rgba(opts.iconColor));
  widgetSetWidth(icon, 32);

  // 标题
  const label = Text(opts.label);
  textSetFontSize(label, 15);
  textSetColor(label, ...rgba(opts.isSelected ? c.textAccent : c.textPrimary));
  if (opts.isSelected) textSetFontWeight(label, 15, 600);

  // 角标
  const badge = Text(String(opts.badgeCount));
  textSetFontSize(badge, 13);
  textSetColor(badge, ...rgba(c.textSecondary));
  widgetSetHidden(badge, opts.badgeCount === 0 ? 1 : 0);

  // 右箭头（mobile 惯例）
  const chevron = Text('›');
  textSetFontSize(chevron, 20);
  textSetColor(chevron, ...rgba(c.textDisabled));

  const row = HStack(12, [icon, label, Spacer(), badge, chevron]);
  widgetMatchParentWidth(row);
  widgetSetHeight(row, 48);
  setPadding(row, 0, 16, 0, 16);
  setCornerRadius(row, 8);

  // 背景：选中 vs 普通
  widgetSetBackgroundColor(
    row,
    ...(opts.isSelected ? rgba(c.sidebarItemSelected) : rgba(c.bgBase))
  );

  widgetSetOnClick(row, opts.onTap);

  // Hover（平板/指针设备）
  widgetSetOnHover(row, (enter: boolean) => {
    const nc = theme();
    if (!opts.isSelected) {
      widgetSetBackgroundColor(row, ...(enter ? rgba(nc.bgHover) : rgba(nc.bgBase)));
    }
  });

  return row;
}

// ============================================================
// 新建列表输入行（移动端）
// ============================================================

function buildMobileCreateRow(
  container: any,
  onSelectList: (id: string) => void
): any {
  const c = theme();

  const colorDot = Text('●');
  textSetFontSize(colorDot, 18);
  textSetColor(colorDot, ...rgba(COLOR_ACCENT));
  widgetSetWidth(colorDot, 32);

  _inputField = TextField('列表名称…', (_v: string) => {});
  textfieldSetBorderless(_inputField, 1);
  textfieldSetFontSize(_inputField, 15);
  widgetMatchParentWidth(_inputField);

  const confirmBtn = Button('添加', () => mobileConfirmCreate(container, onSelectList));
  buttonSetBordered(confirmBtn, 0);
  buttonSetTextColor(confirmBtn, ...rgba(COLOR_ACCENT));
  textSetFontSize(confirmBtn, 14);

  textfieldSetOnSubmit(_inputField, () => mobileConfirmCreate(container, onSelectList));

  const row = HStack(12, [colorDot, _inputField, confirmBtn]);
  widgetMatchParentWidth(row);
  widgetSetHeight(row, 52);
  setPadding(row, 0, 16, 0, 12);
  setCornerRadius(row, 10);
  widgetSetBackgroundColor(row, ...rgba(c.bgCard));

  return row;
}

function mobileConfirmCreate(
  container: any,
  onSelectList: (id: string) => void
): void {
  const name = textfieldGetString(_inputField).trim();
  if (!name) {
    mobileCancelCreate();
    return;
  }
  const list = createList(name);
  textfieldSetString(_inputField, '');
  _isCreating = false;
  widgetSetHidden(_inputRow, 1);
  // 立即切换到新列表
  selectList(list.id);
  loadCurrentTasks();
  onSelectList(list.id);
}

function mobileCancelCreate(): void {
  textfieldSetString(_inputField, '');
  _isCreating = false;
  widgetSetHidden(_inputRow, 1);
}

// ============================================================
// 新建列表底部按钮
// ============================================================

function buildMobileNewListButton(inputRow: any): any {
  const c = theme();

  const plusIcon = Text('+');
  textSetFontSize(plusIcon, 20);
  textSetColor(plusIcon, ...rgba(c.textAccent));

  const btnLabel = Text('新建列表');
  textSetFontSize(btnLabel, 15);
  textSetColor(btnLabel, ...rgba(c.textAccent));

  const btn = HStack(10, [plusIcon, btnLabel]);
  widgetMatchParentWidth(btn);
  widgetSetHeight(btn, 50);
  setPadding(btn, 0, 16, 0, 16);
  widgetSetBackgroundColor(btn, ...rgba(c.bgBase));

  widgetSetOnClick(btn, () => {
    if (_isCreating) {
      mobileCancelCreate();
    } else {
      _isCreating = true;
      widgetSetHidden(inputRow, 0);
      textfieldFocus(_inputField);
    }
  });

  return btn;
}

// ============================================================
// 工具：细分割线
// ============================================================

function buildMobileDivider(): any {
  const c    = theme();
  const line = Divider();
  widgetMatchParentWidth(line);
  widgetSetHeight(line, 1);
  widgetSetBackgroundColor(line, ...rgba(c.divider));
  return line;
}
