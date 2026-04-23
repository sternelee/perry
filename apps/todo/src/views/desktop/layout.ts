// ============================================================
// 桌面端三栏布局
//
// 布局结构：
//   ┌──────────┬──────────────┬──────────────────┐
//   │ Sidebar  │  Task List   │  Detail Panel    │
//   │  220px   │    320px     │    flex (≥380px) │
//   └──────────┴──────────────┴──────────────────┘
//
// 各栏完成进度：
//   Step 3 ✅  左栏：真实侧边栏（智能列表 + 自定义列表 + 新建）
//   Step 4 ✅  中栏：任务列表（活跃 + 已完成折叠 + 快速输入）
//   Step 5 ⏳  右栏：详情面板（占位，Step 5 实现）
// ============================================================

import {
  HStack,
  VStack,
  Text,
  widgetSetWidth,
  widgetMatchParentHeight,
  widgetMatchParentWidth,
  widgetSetBackgroundColor,
  textSetColor,
  textSetFontSize,
} from 'perry/ui';
import { theme, rgba }            from '../../theme/colors';
import { registerRefresh }        from '../../state/app-state';
import { buildSidebar }           from '../sidebar/index';
import { buildTaskListPanel }     from '../task-list/index';

/** 三个主要面板的 Widget 句柄（供跨模块刷新访问） */
export interface DesktopPanels {
  root:     any;  // 整个桌面布局的根 Widget（传给 App body）
  sidebar:  any;  // 左栏（侧边栏实例）
  taskList: any;  // 中栏容器（Step 4 填充）
  detail:   any;  // 右栏容器（Step 5 填充）
}

/**
 * 构建桌面端三栏布局
 */
export function buildDesktopLayout(): DesktopPanels {
  const c = theme();

  // ── 左栏：侧边栏（Step 3 ✅ 真实内容）────────────────────
  const sidebar = buildSidebar();
  widgetSetWidth(sidebar, 220);
  widgetMatchParentHeight(sidebar);

  // ── 中栏：任务列表（Step 4 ✅ 真实内容）─────────────────
  const taskList = buildTaskListPanel();
  widgetSetWidth(taskList, 320);
  widgetMatchParentHeight(taskList);

  // ── 右栏：详情面板（Step 5 占位）─────────────────────────
  const detailPlaceholder = Text('详情面板\n  Step 5 实现\n\n←  点击任务后展开');
  textSetColor(detailPlaceholder, ...rgba(c.textDisabled));
  textSetFontSize(detailPlaceholder, 12);

  const detail = VStack(0, [detailPlaceholder]);
  widgetMatchParentHeight(detail);
  widgetMatchParentWidth(detail);
  widgetSetBackgroundColor(detail, ...rgba(c.bgSurface));

  // ── 三栏组合 ──────────────────────────────────────────────
  // Perry 的 SplitView 仅支持两栏；
  // 三栏用 HStack 实现固定宽度分割（后续可换 SplitView 嵌套以支持拖拽调整）
  const root = HStack(0, [sidebar, taskList, detail]);
  widgetMatchParentWidth(root);
  widgetMatchParentHeight(root);
  widgetSetBackgroundColor(root, ...rgba(c.bgBase));

  // ── 注册主题刷新回调 ──────────────────────────────────────
  // 侧边栏和任务列表有各自的刷新回调，此处只更新右栏背景色
  registerRefresh('desktop-layout-theme', () => {
    const nc = theme();
    widgetSetBackgroundColor(detail,   ...rgba(nc.bgSurface));
    widgetSetBackgroundColor(root,     ...rgba(nc.bgBase));
    textSetColor(detailPlaceholder,    ...rgba(nc.textDisabled));
  });

  return { root, sidebar, taskList, detail };
}
