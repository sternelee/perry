// ============================================================
// 业务服务层：任务服务（task-service.ts）
//
// 职责（高于纯查询层）：
//   1. 封装"数据库写入 + 内存缓存更新 + UI 刷新触发"三步操作
//   2. 实现业务规则（如完成任务后的排序下沉逻辑）
//   3. 提供"加载当前列表任务"的统一入口（根据 selectedListId 路由到正确查询）
//
// 使用方式：UI 层只调用此文件中的函数，不直接访问 db/ 查询层。
// ============================================================

import {
  dbCreateTask,
  dbGetTask,
  dbGetTasksByList,
  dbGetMyDayTasks,
  dbGetImportantTasks,
  dbGetPlannedTasks,
  dbGetAllTasks,
  dbUpdateTaskTitle,
  dbUpdateTaskNotes,
  dbToggleTaskDone,
  dbToggleTaskImportant,
  dbToggleTaskMyDay,
  dbSetTaskDueDate,
  dbMoveTask,
  dbDeleteTask,
  dbDeleteCompletedInList,
  dbResetExpiredMyDay,
} from '../db/task-queries';

import {
  dbGetStepsByTask,
  dbCreateStep,
  dbToggleStepDone,
  dbUpdateStepTitle,
  dbDeleteStep,
  dbGetStepProgress,
} from '../db/step-queries';

import {
  appState,
  triggerRefresh,
  triggerRefreshAll,
} from '../state/app-state';

import { generateId }  from '../utils/id';
import { SMART_LIST, Task, Step, ListSelection, isSmartList } from '../types/index';
import { todayStr }    from '../utils/date';

// ============================================================
// 任务列表加载（根据 selectedListId 路由）
// ============================================================

/**
 * 根据当前 selectedListId 从数据库加载对应的任务列表，
 * 写入 appState.tasks，并触发 task-list 视图刷新。
 *
 * 这是任务列表视图刷新的核心入口，切换列表、增删改完成后都应调用。
 */
export function loadCurrentTasks(): void {
  const id = appState.selectedListId;

  if (id === SMART_LIST.MY_DAY) {
    appState.tasks = dbGetMyDayTasks();
  } else if (id === SMART_LIST.IMPORTANT) {
    appState.tasks = dbGetImportantTasks();
  } else if (id === SMART_LIST.PLANNED) {
    appState.tasks = dbGetPlannedTasks();
  } else if (id === SMART_LIST.ALL) {
    appState.tasks = dbGetAllTasks();
  } else {
    // 自定义列表
    appState.tasks = dbGetTasksByList(id);
  }

  triggerRefresh('task-list');
}

// ============================================================
// 任务 CRUD（带缓存更新 + UI 刷新）
// ============================================================

/**
 * 创建新任务并刷新视图
 *
 * @param title   任务标题
 * @param listId  所属列表 ID（若为 null，使用当前选中的自定义列表）
 * @param options 可选属性（isMyDay / isImportant / dueDate）
 * @returns       新建的 Task 对象
 */
export function createTask(
  title: string,
  listId?: string,
  options: {
    isMyDay?:     boolean;
    isImportant?: boolean;
    dueDate?:     number | null;
  } = {}
): Task {
  // 若当前在智能列表，新任务归入第一个自定义列表（或默认列表）
  const targetListId = listId ??
    (isSmartList(appState.selectedListId)
      ? (appState.lists[0]?.id ?? 'list:tasks')
      : appState.selectedListId);

  // 在"我的一天"视图创建任务时自动加入今天
  const isMyDay = options.isMyDay ??
    (appState.selectedListId === SMART_LIST.MY_DAY);

  // 在"重要"视图创建任务时自动标星
  const isImportant = options.isImportant ??
    (appState.selectedListId === SMART_LIST.IMPORTANT);

  const task = dbCreateTask(generateId(), title, targetListId, {
    isMyDay,
    isImportant,
    dueDate: options.dueDate,
  });

  // 若新任务应在当前视图显示，插入缓存头部（未完成排最前）
  if (shouldShowInCurrentView(task)) {
    appState.tasks = [task, ...appState.tasks.filter(t => !t.isDone)].concat(
      appState.tasks.filter(t => t.isDone)
    );
  }

  triggerRefreshAll(['task-list', 'sidebar']); // 侧边栏计数也要更新
  return task;
}

/**
 * 切换任务完成状态
 *
 * 业务规则：
 * - 标为已完成 → 任务移至列表末尾（已完成分组）
 * - 取消完成   → 任务移回列表顶部（未完成分组）
 */
export function toggleTaskDone(taskId: string): void {
  const task = appState.tasks.find(t => t.id === taskId);
  if (!task) return;

  const newDone = !task.isDone;
  dbToggleTaskDone(taskId, newDone);

  // 更新内存缓存中对应项
  task.isDone = newDone;

  // 重新排序：未完成在前，已完成在后
  appState.tasks = [
    ...appState.tasks.filter(t => !t.isDone),
    ...appState.tasks.filter(t => t.isDone),
  ];

  triggerRefreshAll(['task-list', 'sidebar']);
}

/**
 * 切换任务"重要"（星标）状态
 *
 * 若当前在"重要"智能列表，取消星标后任务从视图中移除。
 */
export function toggleTaskImportant(taskId: string): void {
  const task = appState.tasks.find(t => t.id === taskId)
    ?? dbGetTask(taskId); // 可能在详情面板中操作当前列表外的任务
  if (!task) return;

  const newVal = !task.isImportant;
  dbToggleTaskImportant(taskId, newVal);

  if (task) task.isImportant = newVal;

  // "重要"列表：取消星标后移出视图
  if (appState.selectedListId === SMART_LIST.IMPORTANT && !newVal) {
    appState.tasks = appState.tasks.filter(t => t.id !== taskId);
  }

  triggerRefreshAll(['task-list', 'detail']);
}

/**
 * 切换"加入/移除我的一天"
 *
 * "我的一天"列表：移除标记后任务从视图中消失。
 */
export function toggleTaskMyDay(taskId: string): void {
  const task = appState.tasks.find(t => t.id === taskId)
    ?? dbGetTask(taskId);
  if (!task) return;

  const newVal = !task.isMyDay;
  dbToggleTaskMyDay(taskId, newVal);

  if (task) {
    task.isMyDay    = newVal;
    task.myDayDate  = newVal ? todayStr() : null;
  }

  if (appState.selectedListId === SMART_LIST.MY_DAY && !newVal) {
    appState.tasks = appState.tasks.filter(t => t.id !== taskId);
  }

  triggerRefreshAll(['task-list', 'detail']);
}

/**
 * 更新任务标题
 */
export function updateTaskTitle(taskId: string, title: string): void {
  dbUpdateTaskTitle(taskId, title);
  const task = appState.tasks.find(t => t.id === taskId);
  if (task) task.title = title;
  triggerRefresh('task-list');
}

/**
 * 更新任务备注
 */
export function updateTaskNotes(taskId: string, notes: string): void {
  dbUpdateTaskNotes(taskId, notes);
  const task = appState.tasks.find(t => t.id === taskId);
  if (task) task.notes = notes;
  // 备注只在详情面板显示，不触发 task-list 刷新
}

/**
 * 设置或清除截止日期
 */
export function setTaskDueDate(taskId: string, dueDate: number | null): void {
  dbSetTaskDueDate(taskId, dueDate);
  const task = appState.tasks.find(t => t.id === taskId);
  if (task) task.dueDate = dueDate;

  // "计划内"列表：清除日期后任务应移出视图
  if (appState.selectedListId === SMART_LIST.PLANNED && dueDate === null) {
    appState.tasks = appState.tasks.filter(t => t.id !== taskId);
  } else {
    // 计划内列表需要重新排序（按 dueDate ASC）
    if (appState.selectedListId === SMART_LIST.PLANNED) {
      appState.tasks = appState.tasks.slice().sort(
        (a, b) => (a.dueDate ?? 0) - (b.dueDate ?? 0)
      );
    }
  }

  triggerRefreshAll(['task-list', 'detail']);
}

/**
 * 删除任务
 *
 * 同时关闭详情面板（若被删除的任务当前正在展示）。
 */
export function deleteTask(taskId: string): void {
  dbDeleteTask(taskId);
  appState.tasks = appState.tasks.filter(t => t.id !== taskId);

  // 若删除的是当前打开的任务，关闭详情面板
  if (appState.selectedTaskId === taskId) {
    appState.selectedTaskId = null;
    appState.isDetailOpen   = false;
    appState.currentSteps   = [];
  }

  triggerRefreshAll(['task-list', 'detail', 'sidebar']);
}

/**
 * 清空当前列表内所有已完成任务
 */
export function clearCompletedTasks(): void {
  const id = appState.selectedListId;

  if (isSmartList(id)) {
    // 智能列表：逐条删除（没有单一 listId 可批量删除）
    const completedIds = appState.tasks
      .filter(t => t.isDone)
      .map(t => t.id);
    completedIds.forEach(dbDeleteTask);
  } else {
    dbDeleteCompletedInList(id);
  }

  appState.tasks = appState.tasks.filter(t => !t.isDone);
  triggerRefreshAll(['task-list', 'sidebar']);
}

// ============================================================
// 步骤 CRUD（详情面板使用）
// ============================================================

/**
 * 加载当前选中任务的步骤列表，写入 appState.currentSteps
 */
export function loadCurrentSteps(): void {
  if (!appState.selectedTaskId) {
    appState.currentSteps = [];
    return;
  }
  appState.currentSteps = dbGetStepsByTask(appState.selectedTaskId);
  triggerRefresh('detail');
}

/**
 * 为当前任务添加新步骤
 */
export function createStep(title: string): Step | null {
  if (!appState.selectedTaskId) return null;

  const step = dbCreateStep(generateId(), title, appState.selectedTaskId);
  // 新步骤追加到未完成步骤末尾
  appState.currentSteps = [
    ...appState.currentSteps.filter(s => !s.isDone),
    step,
    ...appState.currentSteps.filter(s => s.isDone),
  ];
  triggerRefresh('detail');
  return step;
}

/**
 * 切换步骤完成状态（完成后下沉到已完成区）
 */
export function toggleStepDone(stepId: string): void {
  const step = appState.currentSteps.find(s => s.id === stepId);
  if (!step) return;

  const newDone = !step.isDone;
  dbToggleStepDone(stepId, newDone);
  step.isDone = newDone;

  // 重新排序
  appState.currentSteps = [
    ...appState.currentSteps.filter(s => !s.isDone),
    ...appState.currentSteps.filter(s => s.isDone),
  ];
  triggerRefresh('detail');
}

/**
 * 修改步骤标题
 */
export function updateStepTitle(stepId: string, title: string): void {
  dbUpdateStepTitle(stepId, title);
  const step = appState.currentSteps.find(s => s.id === stepId);
  if (step) step.title = title;
  triggerRefresh('detail');
}

/**
 * 删除步骤
 */
export function deleteStep(stepId: string): void {
  dbDeleteStep(stepId);
  appState.currentSteps = appState.currentSteps.filter(s => s.id !== stepId);
  triggerRefresh('detail');
}

/**
 * 获取当前任务的步骤进度（供任务卡片显示"x/y 步骤"）
 */
export function getStepProgress(taskId: string): { total: number; done: number } {
  return dbGetStepProgress(taskId);
}

// ============================================================
// 启动时跨天重置（"我的一天"）
// ============================================================

/**
 * 清除所有过期的"我的一天"标记（应用启动时调用一次）
 *
 * @returns 被重置的任务数量（0 表示没有过期项，通常跳过日志）
 */
export function resetExpiredMyDay(): number {
  const count = dbResetExpiredMyDay();
  if (count > 0) {
    // 若当前正在显示"我的一天"，刷新列表
    if (appState.selectedListId === SMART_LIST.MY_DAY) {
      loadCurrentTasks();
    }
  }
  return count;
}

// ============================================================
// 内部工具
// ============================================================

/**
 * 判断一个任务是否应在当前选中视图中显示
 * 用于新建任务后决定是否立即插入内存缓存
 */
function shouldShowInCurrentView(task: Task): boolean {
  const id = appState.selectedListId;
  if (id === SMART_LIST.MY_DAY)    return task.isMyDay;
  if (id === SMART_LIST.IMPORTANT) return task.isImportant;
  if (id === SMART_LIST.PLANNED)   return task.dueDate !== null;
  if (id === SMART_LIST.ALL)       return true;
  return task.listId === id;
}
