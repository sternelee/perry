// ============================================================
// 全局应用状态管理
//
// Perry 没有虚拟 DOM，状态变更必须手动触发 UI 刷新。
// 本模块实现一个轻量的"发布-订阅"刷新机制：
//
//   1. appState   —— 单一可变状态对象（全局唯一）
//   2. registerRefresh(key, fn) —— 各视图注册自己的刷新函数
//   3. triggerRefresh(key) —— 状态变更后通知对应视图重绘
//   4. 高层辅助函数（selectList / selectTask / toggleDark）
//      封装"改状态 + 触发刷新"的双步操作，保证不遗漏
// ============================================================

import { ListSelection, Task, TaskList, Step, SMART_LIST, LayoutMode } from '../types/index';

// ============================================================
// 状态类型定义
// ============================================================

/** 全局应用状态快照 */
export interface AppState {
  // ── 导航状态 ───────────────────────────────────────────
  /** 当前选中的列表 ID（智能列表或自定义列表） */
  selectedListId: ListSelection;

  /** 当前选中的任务 ID（null 表示未选中任何任务） */
  selectedTaskId: string | null;

  /** 详情面板是否展开（桌面：右侧滑入；移动：推入新页面） */
  isDetailOpen: boolean;

  // ── 主题 ────────────────────────────────────────────────
  /** 当前是否为深色模式 */
  isDark: boolean;

  // ── 布局 ────────────────────────────────────────────────
  /** 运行时检测的布局模式（getDeviceIdiom() 决定） */
  layout: LayoutMode;

  // ── 数据缓存 ────────────────────────────────────────────
  // 从数据库读取后缓存在内存，减少重复 IO。
  // 规则：修改数据库后必须同步更新此缓存，然后触发对应刷新。

  /** 所有自定义列表（侧边栏导航用） */
  lists: TaskList[];

  /** 当前列表视图下的任务（由 selectedListId 过滤决定） */
  tasks: Task[];

  /** 当前选中任务的子步骤（打开详情面板时懒加载） */
  currentSteps: Step[];

  // ── UI 临时状态 ─────────────────────────────────────────
  /** 侧边栏在移动端是否展开（抽屉模式） */
  isSidebarOpen: boolean;

  /** 快速输入框是否聚焦（用于控制占位符显示） */
  isInputFocused: boolean;

  /** 已完成任务是否展开显示（true = 展开，false = 折叠） */
  isCompletedExpanded: boolean;
}

// ============================================================
// 全局状态单例
// ============================================================

/** 全局可变状态（全应用唯一实例） */
export const appState: AppState = {
  selectedListId: SMART_LIST.MY_DAY,
  selectedTaskId: null,
  isDetailOpen: false,

  isDark: false,
  layout: 'desktop',

  lists: [],
  tasks: [],
  currentSteps: [],

  isSidebarOpen: false,
  isInputFocused: false,
  isCompletedExpanded: false,
};

// ============================================================
// 刷新回调注册表（发布-订阅）
// ============================================================

/**
 * 视图刷新回调 Map
 * key: 视图标识符（如 'sidebar' | 'task-list' | 'detail' | 'theme'）
 * value: 该视图的刷新函数
 */
const _refreshCallbacks: Map<string, () => void> = new Map();

/**
 * 注册一个视图的刷新回调
 *
 * @param key      视图标识符，建议使用 kebab-case 字符串
 * @param callback 视图刷新函数（通常是"清空容器 + 重新渲染"）
 *
 * @example
 * // 在 sidebar 视图构建时注册
 * registerRefresh('sidebar', () => {
 *   widgetClearChildren(sidebarContainer);
 *   buildSidebarItems(sidebarContainer);
 * });
 */
export function registerRefresh(key: string, callback: () => void): void {
  _refreshCallbacks.set(key, callback);
}

/**
 * 注销一个视图的刷新回调（视图销毁时调用，避免内存泄漏）
 */
export function unregisterRefresh(key: string): void {
  _refreshCallbacks.delete(key);
}

/**
 * 触发指定视图的刷新
 * 若该 key 未注册，静默忽略（避免初始化顺序问题）
 */
export function triggerRefresh(key: string): void {
  const cb = _refreshCallbacks.get(key);
  if (cb) cb();
}

/**
 * 触发多个视图的刷新（批量通知）
 */
export function triggerRefreshAll(keys: string[]): void {
  for (const key of keys) {
    triggerRefresh(key);
  }
}

/**
 * 触发所有已注册视图的刷新
 * 通常在主题切换时使用（所有颜色都需要更新）
 */
export function triggerGlobalRefresh(): void {
  _refreshCallbacks.forEach((cb) => cb());
}

// ============================================================
// 高层状态变更操作
// 每个操作封装"改状态 + 触发刷新"，外部只调用这些函数
// ============================================================

/**
 * 切换选中列表
 * - 清除当前选中任务
 * - 关闭详情面板
 * - 通知侧边栏更新选中高亮，通知任务列表加载新数据
 *
 * @param listId 目标列表 ID（智能列表或自定义列表）
 */
export function selectList(listId: ListSelection): void {
  appState.selectedListId = listId;
  appState.selectedTaskId = null;
  appState.isDetailOpen = false;
  appState.isCompletedExpanded = false;
  // 注意：此处不重新加载 tasks 数据，由 task-list 视图的刷新函数负责查询
  triggerRefreshAll(['sidebar', 'task-list', 'detail']);
}

/**
 * 选中（或取消选中）一个任务，控制详情面板的开关
 *
 * @param taskId 目标任务 ID，传 null 则关闭详情面板
 */
export function selectTask(taskId: string | null): void {
  appState.selectedTaskId = taskId;
  appState.isDetailOpen = taskId !== null;
  appState.currentSteps = []; // 清空旧步骤，由详情面板刷新时重新加载
  triggerRefreshAll(['task-list', 'detail']);
}

/**
 * 切换亮/暗色模式并触发全局重绘
 *
 * @param isDark true = 深色模式，false = 浅色模式
 */
export function setDarkMode(isDark: boolean): void {
  appState.isDark = isDark;
  triggerGlobalRefresh();
}

/**
 * 在移动端切换侧边栏抽屉的展开/收起
 */
export function toggleSidebar(): void {
  appState.isSidebarOpen = !appState.isSidebarOpen;
  triggerRefresh('sidebar');
}

/**
 * 切换"已完成任务"的折叠/展开状态
 */
export function toggleCompletedExpanded(): void {
  appState.isCompletedExpanded = !appState.isCompletedExpanded;
  triggerRefresh('task-list');
}
