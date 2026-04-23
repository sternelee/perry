// ============================================================
// 侧边栏组件：可复用的列表项构建工具
//
// Perry 没有 CSS，所有视觉状态（选中/hover/普通）必须通过
// widgetSetBackgroundColor + widgetSetOnHover + widgetSetOnClick
// 手动管理。
//
// 每个列表项的结构：
//   HStack [
//     图标 Text (24px)  ←  SF Symbol fallback 文本
//     标题 Text (flex)
//     角标 Text (数字) ← count > 0 时显示
//   ]
//
// 状态机（每个 item 独立维护）：
//   normal → hover → selected
//   selected 状态下 hover 仍保持 selected 背景色
// ============================================================

import {
  HStack,
  VStack,
  Text,
  Button,
  Spacer,
  widgetAddChild,
  widgetClearChildren,
  widgetSetBackgroundColor,
  widgetSetOnClick,
  widgetSetOnHover,
  widgetSetOnDoubleClick,
  widgetMatchParentWidth,
  widgetSetHeight,
  widgetSetWidth,
  widgetSetHidden,
  setCornerRadius,
  setPadding,
  textSetColor,
  textSetFontSize,
  textSetFontWeight,
  widgetSetContextMenu,
  menuCreate,
  menuAddItem,
  menuAddSeparator,
  alert,
} from 'perry/ui';

import { theme, rgba, Color }     from '../../theme/colors';
import { appState, selectList,
         registerRefresh }        from '../../state/app-state';
import { ListSelection,
         SMART_LIST_META,
         SmartListMeta }          from '../../types/index';
import { getListBadgeCounts,
         renameList, deleteList,
         setListColor }           from '../../services/list-service';
import { loadCurrentTasks }       from '../../services/task-service';
import { TaskList, LIST_COLOR_PRESETS } from '../../types/index';
import { LIST_COLOR_PRESETS as COLOR_PRESETS } from '../../theme/colors';

// ============================================================
// 内部：选中项状态追踪
// 记录当前选中列表项对应的背景 Widget，用于取消选中时还原
// ============================================================
let _selectedItemBg: any = null;
let _hoveredItemBg:  any = null;

/**
 * 将上一个选中项背景还原为 normal 颜色
 * 并将新选中项背景设为 selected 颜色
 */
function setSelectedItem(newBg: any): void {
  const c = theme();
  // 还原旧选中项
  if (_selectedItemBg && _selectedItemBg !== newBg) {
    widgetSetBackgroundColor(_selectedItemBg, ...rgba(c.bgSidebar));
  }
  _selectedItemBg = newBg;
  widgetSetBackgroundColor(newBg, ...rgba(c.sidebarItemSelected));
}

// ============================================================
// 构建单个列表项
// ============================================================

export interface SidebarItemOptions {
  listId:    ListSelection; // 点击后 selectList 的目标 ID
  icon:      string;        // 图标文本（SF Symbol fallback）
  label:     string;        // 主标题
  badgeCount?: number;      // 角标数字（0 或不传则隐藏）
  accentColor?: Color;      // 图标颜色（默认用 textAccent）
  onContextMenu?: (listId: string) => void; // 右键菜单回调
}

/**
 * 构建一个侧边栏列表项 Widget
 *
 * 返回 { row, bg } 其中 bg 是背景容器（用于选中状态管理）
 */
export function buildSidebarItem(opts: SidebarItemOptions): { row: any; bg: any } {
  const c = theme();
  const isSelected = appState.selectedListId === opts.listId;

  // ── 图标 ───────────────────────────────────────────────────
  const icon = Text(opts.icon);
  textSetFontSize(icon, 16);
  textSetColor(icon, ...(opts.accentColor ? rgba(opts.accentColor) : rgba(c.textAccent)));
  widgetSetWidth(icon, 28);

  // ── 标题 ───────────────────────────────────────────────────
  const label = Text(opts.label);
  textSetFontSize(label, 14);
  textSetColor(label, ...rgba(c.sidebarItemText));
  if (isSelected) textSetFontWeight(label, 14, 600); // 选中时加粗

  // ── 角标（未完成数量） ──────────────────────────────────────
  const badge = Text(String(opts.badgeCount ?? 0));
  textSetFontSize(badge, 12);
  textSetColor(badge, ...rgba(c.textSecondary));
  widgetSetHidden(badge, !opts.badgeCount || opts.badgeCount === 0 ? 1 : 0);

  // ── 行容器 ─────────────────────────────────────────────────
  const row = HStack(8, [icon, label, Spacer(), badge]);
  widgetMatchParentWidth(row);
  widgetSetHeight(row, 36);
  setPadding(row, 0, 12, 0, 12);

  // ── 背景容器（用于选中/hover 颜色切换） ───────────────────
  const bg = VStack(0, [row]);
  widgetMatchParentWidth(bg);
  widgetSetHeight(bg, 38);
  setCornerRadius(bg, 6);
  setPadding(bg, 1, 4, 1, 4);

  // 初始背景色
  widgetSetBackgroundColor(
    bg,
    ...(isSelected ? rgba(c.sidebarItemSelected) : rgba(c.bgSidebar))
  );
  if (isSelected) _selectedItemBg = bg;

  // ── 交互事件 ───────────────────────────────────────────────

  // Hover：进入时变亮，离开时还原（不覆盖选中态）
  widgetSetOnHover(bg, (isEnter: boolean) => {
    const nc = theme();
    if (bg === _selectedItemBg) return; // 选中态不响应 hover
    if (isEnter) {
      widgetSetBackgroundColor(bg, ...rgba(nc.sidebarItemHover));
      _hoveredItemBg = bg;
    } else {
      widgetSetBackgroundColor(bg, ...rgba(nc.bgSidebar));
      if (_hoveredItemBg === bg) _hoveredItemBg = null;
    }
  });

  // Click：切换选中列表
  widgetSetOnClick(bg, () => {
    setSelectedItem(bg);
    selectList(opts.listId);
    loadCurrentTasks();
  });

  return { row, bg };
}

// ============================================================
// 构建列表项的右键菜单（自定义列表专用）
// ============================================================

/**
 * 为自定义列表项挂载右键菜单（重命名 / 修改颜色 / 删除）
 */
export function attachListContextMenu(
  bg: any,
  list: TaskList,
  onMutated: () => void
): void {
  const menu = menuCreate();

  // ── 重命名 ────────────────────────────────────────────────
  menuAddItem(menu, '重命名列表', () => {
    // Perry 暂无内联文本编辑框，用 alert 提示操作路径
    // Step 4 完成后替换为内联重命名输入框
    alert('重命名', `请在任务列表顶部的列表名称处双击进行重命名。`);
  });

  // ── 修改颜色 ──────────────────────────────────────────────
  menuAddItem(menu, '修改颜色', () => {
    // 循环切换预设色（简化实现，完整色板选择器在后续 UI 优化中添加）
    const presets = COLOR_PRESETS;
    const currentIdx = presets.findIndex(p =>
      Math.abs(p.color.r - list.colorR) < 0.01
    );
    const nextColor = presets[(currentIdx + 1) % presets.length].color;
    setListColor(list.id, nextColor);
    onMutated();
  });

  menuAddSeparator(menu);

  // ── 删除列表 ──────────────────────────────────────────────
  menuAddItem(menu, `删除列表`, () => {
    const affected = deleteList(list.id);
    // deleteList 内部已调用 selectList(MY_DAY) 并触发 sidebar 刷新
    // 若有受影响任务，在实际应用中应先弹确认框（Perry alert 为同步阻塞）
    if (affected > 0) {
      alert('已删除', `列表"${list.name}"及其 ${affected} 条任务已被删除。`);
    }
  });

  widgetSetContextMenu(bg, menu);
}
