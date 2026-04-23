// ============================================================
// 移动端布局
//
// 移动端布局采用底部 TabBar + NavStack 组合：
//
//   ┌─────────────────────┐
//   │                     │
//   │   内容区（NavStack） │
//   │   根据选中 Tab 切换  │
//   │                     │
//   ├─────────────────────┤
//   │ 今天 │ 重要 │ 全部 │列表│  ← TabBar（底部导航）
//   └─────────────────────┘
//
// Tab 内容完成进度：
//   Tab 0 (我的一天) ⏳  Step 4 实现任务列表视图
//   Tab 1 (重要)     ⏳  Step 4 实现任务列表视图
//   Tab 2 (全部)     ⏳  Step 4 实现任务列表视图
//   Tab 3 (列表)     ✅  Step 3 实现：列表浏览器（智能+自定义列表）
// ============================================================

import {
  NavStack,
  TabBar,
  VStack,
  Text,
  tabbarAddTab,
  navstackPush,
  navstackPop,
  widgetMatchParentHeight,
  widgetMatchParentWidth,
  widgetSetBackgroundColor,
  textSetColor,
  textSetFontSize,
} from 'perry/ui';
import { theme, rgba }                   from '../../theme/colors';
import { appState, selectList,
         registerRefresh }               from '../../state/app-state';
import { SMART_LIST, SmartListId }       from '../../types/index';
import { buildMobileListsTab }           from './lists-tab';
import { buildTaskListPanel }            from '../task-list/index';

// ============================================================
// Tab 定义
// ============================================================

interface TabConfig {
  label:  string;
  listId: string;
}

const TAB_CONFIGS: TabConfig[] = [
  { label: '我的一天', listId: SMART_LIST.MY_DAY },
  { label: '重要',     listId: SMART_LIST.IMPORTANT },
  { label: '全部',     listId: SMART_LIST.ALL },
  { label: '列表',     listId: 'mobile:lists' },
];

// ============================================================
// 当前活跃 Tab 索引（用于 push/pop 定向到正确的 NavStack）
// ============================================================
let _activeTabIndex = 0;

// ============================================================
// 导出的布局句柄
// ============================================================

export interface MobilePanels {
  root:      any;    // TabBar 根 Widget
  navStacks: any[];  // 各 Tab 的 NavStack 句柄
}

/**
 * 构建移动端 TabBar + NavStack 布局
 *
 * 返回根 Widget，传给 App({ body: ... })
 */
export function buildMobileLayout(): MobilePanels {
  const c = theme();
  const navStacks: any[] = [];

  // ── 为 Tab 0-2 推入真实任务列表视图（Step 4 ✅）─────────
  for (let i = 0; i < 3; i++) {
    const cfg = TAB_CONFIGS[i];

    // 每个 Tab 共用同一个 buildTaskListPanel 实例
    // （Perry 中同一 Widget 不能挂到多个父节点，需各自独立构建）
    const taskPanel = buildTaskListPanel();
    widgetMatchParentWidth(taskPanel);
    widgetMatchParentHeight(taskPanel);

    const nav = NavStack();
    navstackPush(nav, taskPanel, cfg.label);
    navStacks.push(nav);
  }

  // ── Tab 3：列表浏览器（Step 3 ✅）─────────────────────────
  const listsTab = buildMobileListsTab((listId: string) => {
    // 用户点击列表项后：如果是智能列表，切换到对应的 Tab（0/1/2）；
    // 如果是自定义列表，切换到 Tab 0（我的一天 Tab 暂用，Step 4 后改为独立页面）
    const tabMap: Record<string, number> = {
      [SMART_LIST.MY_DAY]:    0,
      [SMART_LIST.IMPORTANT]: 1,
      [SMART_LIST.ALL]:       2,
    };
    const targetTab = tabMap[listId] ?? 0;
    // 切换 Tab 需要宿主 TabBar 配合，此处仅更新 activeTab 状态
    // 实际 Tab 切换由 tabbarSetSelectedIndex（Perry API）完成
    // 在 Step 4 实现任务列表视图后，将在此处推入任务列表页
    _activeTabIndex = targetTab;
  });

  const listsNav = NavStack();
  navstackPush(listsNav, listsTab, '列表');
  navStacks.push(listsNav);

  // ── TabBar 主容器 ────────────────────────────────────────
  const tabs = TabBar((index: number) => {
    _activeTabIndex = index;
    const listId = TAB_CONFIGS[index]?.listId ?? SMART_LIST.MY_DAY;
    if (listId !== 'mobile:lists') {
      selectList(listId as SmartListId);
    }
  });

  for (let i = 0; i < TAB_CONFIGS.length; i++) {
    tabbarAddTab(tabs, TAB_CONFIGS[i].label, navStacks[i]);
  }

  widgetMatchParentWidth(tabs);
  widgetMatchParentHeight(tabs);

  // ── 注册主题刷新回调 ─────────────────────────────────────
  registerRefresh('mobile-layout-theme', () => {
    const nc = theme();
    // Tab 0-2 的占位内容背景随主题更新
    for (let i = 0; i < 3; i++) {
      // 各自的内容视图在 Step 4 实现后会注册自己的刷新回调
    }
  });

  // 将 navStacks 导出到模块级变量供跨模块使用
  mobileNavStacks = navStacks;

  return { root: tabs, navStacks };
}

// ============================================================
// 移动端导航辅助函数
// ============================================================

/** 各 Tab 的 NavStack 句柄（由 buildMobileLayout 填充） */
export let mobileNavStacks: any[] = [];

/**
 * 在指定 Tab 的 NavStack 中推入新页面
 */
export function mobilePush(view: any, title: string, tabIndex?: number): void {
  const idx = tabIndex ?? _activeTabIndex;
  const nav = mobileNavStacks[idx];
  if (nav) {
    navstackPush(nav, view, title);
  }
}

/**
 * 在指定 Tab 的 NavStack 中弹出当前页面（返回上级）
 */
export function mobilePop(tabIndex?: number): void {
  const idx = tabIndex ?? _activeTabIndex;
  const nav = mobileNavStacks[idx];
  if (nav) {
    navstackPop(nav);
  }
}
