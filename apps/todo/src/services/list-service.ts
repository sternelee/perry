// ============================================================
// 业务服务层：列表服务（list-service.ts）
//
// 职责：
//   1. 封装列表 CRUD 的"数据库 + 缓存 + UI 刷新"三步操作
//   2. 提供侧边栏计数数据（未完成任务数角标）
//   3. 列表删除时的安全确认逻辑（返回受影响任务数以便 UI 弹窗）
// ============================================================

import {
  dbCreateList,
  dbGetAllLists,
  dbGetList,
  dbRenameList,
  dbSetListColor,
  dbDeleteList,
  dbGetListTaskCounts,
  dbGetSmartListCounts,
} from '../db/list-queries';

import {
  appState,
  selectList,
  triggerRefresh,
  triggerRefreshAll,
} from '../state/app-state';

import { generateId }    from '../utils/id';
import { TaskList }      from '../types/index';
import { SMART_LIST }    from '../types/index';
import { todayStr }      from '../utils/date';

// ============================================================
// 列表初始化（应用启动时）
// ============================================================

/**
 * 从数据库加载所有自定义列表，写入 appState.lists
 * 应在 main.ts 启动时调用一次，之后通过增量更新维护缓存。
 */
export function loadAllLists(): void {
  appState.lists = dbGetAllLists();
  triggerRefresh('sidebar');
}

// ============================================================
// 列表 CRUD
// ============================================================

/**
 * 创建新的自定义列表
 *
 * @param name  列表名称
 * @param color 主题色（默认 Microsoft Blue）
 * @returns     新建的 TaskList 对象
 */
export function createList(
  name: string,
  color?: { r: number; g: number; b: number; a: number }
): TaskList {
  const list = dbCreateList(generateId(), name, color);

  // 追加到内存缓存末尾（保持创建顺序）
  appState.lists = [...appState.lists, list];

  triggerRefresh('sidebar');
  return list;
}

/**
 * 重命名列表
 */
export function renameList(listId: string, newName: string): void {
  dbRenameList(listId, newName);

  // 更新内存缓存
  const list = appState.lists.find(l => l.id === listId);
  if (list) list.name = newName;

  triggerRefresh('sidebar');
}

/**
 * 修改列表主题色
 */
export function setListColor(
  listId: string,
  color: { r: number; g: number; b: number; a: number }
): void {
  dbSetListColor(listId, color);

  const list = appState.lists.find(l => l.id === listId);
  if (list) {
    list.colorR = color.r;
    list.colorG = color.g;
    list.colorB = color.b;
    list.colorA = color.a;
  }

  triggerRefresh('sidebar');
}

/**
 * 删除列表（级联删除其下所有任务和步骤）
 *
 * @returns 被删除的任务数量（UI 层据此决定是否弹出确认框）
 */
export function deleteList(listId: string): number {
  // 计算影响任务数（用于 UI 确认弹窗提示）
  const counts  = dbGetListTaskCounts();
  const affected = counts.get(listId) ?? 0;

  dbDeleteList(listId);
  appState.lists = appState.lists.filter(l => l.id !== listId);

  // 若删除的是当前选中列表，切换到"我的一天"
  if (appState.selectedListId === listId) {
    selectList(SMART_LIST.MY_DAY);
  } else {
    triggerRefresh('sidebar');
  }

  return affected;
}

// ============================================================
// 侧边栏计数（角标数字）
// ============================================================

/**
 * 获取所有列表（智能列表 + 自定义列表）的未完成任务计数
 *
 * 返回格式供侧边栏直接使用：
 * {
 *   'smart:my-day':    N,
 *   'smart:important': N,
 *   'smart:planned':   N,
 *   'smart:all':       N,
 *   '<customId>':      N,
 *   ...
 * }
 */
export function getListBadgeCounts(): Record<string, number> {
  const today = todayStr();

  // 智能列表计数（一次查询）
  const smart = dbGetSmartListCounts(today);

  // 自定义列表计数
  const customCounts = dbGetListTaskCounts();

  const result: Record<string, number> = {
    [SMART_LIST.MY_DAY]:    smart.myDay,
    [SMART_LIST.IMPORTANT]: smart.important,
    [SMART_LIST.PLANNED]:   smart.planned,
    [SMART_LIST.ALL]:       smart.all,
  };

  customCounts.forEach((count, listId) => {
    result[listId] = count;
  });

  return result;
}
