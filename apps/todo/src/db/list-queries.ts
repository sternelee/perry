// ============================================================
// 数据库查询层：自定义任务列表（TaskList）CRUD
//
// TaskList 是用户自定义的任务分组容器，类似 Microsoft To Do 的"列表"。
// 智能列表（我的一天/重要/计划内/全部）不存储在此表中，
// 它们是对 tasks 表的动态查询视图。
// ============================================================

import { getDb }    from './database';
import { TaskList } from '../types/index';

// ============================================================
// 行类型
// ============================================================

interface TaskListRow {
  id:         string;
  name:       string;
  color_r:    number;
  color_g:    number;
  color_b:    number;
  color_a:    number;
  created_at: number;
}

function rowToTaskList(row: TaskListRow): TaskList {
  return {
    id:        row.id,
    name:      row.name,
    colorR:    row.color_r,
    colorG:    row.color_g,
    colorB:    row.color_b,
    colorA:    row.color_a,
    createdAt: row.created_at,
  };
}

// ============================================================
// CREATE
// ============================================================

/**
 * 创建一个新的自定义任务列表
 *
 * @param id    由调用方提供的 UUID
 * @param name  列表名称（非空，最长建议 50 字符）
 * @param color 主题色 RGBA（0.0-1.0），默认 Microsoft Blue
 * @returns     新创建的 TaskList 对象
 */
export function dbCreateList(
  id: string,
  name: string,
  color: { r: number; g: number; b: number; a: number } = { r: 0, g: 0.478, b: 0.831, a: 1 }
): TaskList {
  const now = Date.now();

  getDb().prepare(`
    INSERT INTO task_lists (id, name, color_r, color_g, color_b, color_a, created_at)
    VALUES                 (?, ?, ?, ?, ?, ?, ?)
  `).run(id, name, color.r, color.g, color.b, color.a, now);

  return dbGetList(id)!;
}

// ============================================================
// READ
// ============================================================

/**
 * 按 ID 查询单个列表
 */
export function dbGetList(id: string): TaskList | null {
  const row = getDb()
    .prepare('SELECT * FROM task_lists WHERE id = ?')
    .get(id) as TaskListRow | undefined;
  return row ? rowToTaskList(row) : null;
}

/**
 * 查询所有自定义列表，按创建时间升序排列
 */
export function dbGetAllLists(): TaskList[] {
  const rows = getDb().prepare(`
    SELECT * FROM task_lists ORDER BY created_at ASC
  `).all() as TaskListRow[];
  return rows.map(rowToTaskList);
}

/**
 * 查询各列表下未完成任务的数量（用于侧边栏角标显示）
 *
 * @returns Map<listId, count>
 */
export function dbGetListTaskCounts(): Map<string, number> {
  const rows = getDb().prepare(`
    SELECT list_id, COUNT(*) AS count
    FROM   tasks
    WHERE  is_done = 0
    GROUP  BY list_id
  `).all() as Array<{ list_id: string; count: number }>;

  const map = new Map<string, number>();
  for (const row of rows) {
    map.set(row.list_id, row.count);
  }
  return map;
}

/**
 * 查询智能列表的未完成任务数（我的一天/重要/计划内/全部）
 *
 * 一次查询返回所有四个数值，避免多次 I/O。
 */
export function dbGetSmartListCounts(todayDate: string): {
  myDay:     number;
  important: number;
  planned:   number;
  all:       number;
} {
  const db = getDb();

  const myDay = (db.prepare(`
    SELECT COUNT(*) AS c FROM tasks
    WHERE  is_my_day = 1 AND my_day_date = ? AND is_done = 0
  `).get(todayDate) as { c: number }).c;

  const important = (db.prepare(`
    SELECT COUNT(*) AS c FROM tasks WHERE is_important = 1 AND is_done = 0
  `).get() as { c: number }).c;

  const planned = (db.prepare(`
    SELECT COUNT(*) AS c FROM tasks WHERE due_date IS NOT NULL AND is_done = 0
  `).get() as { c: number }).c;

  const all = (db.prepare(`
    SELECT COUNT(*) AS c FROM tasks WHERE is_done = 0
  `).get() as { c: number }).c;

  return { myDay, important, planned, all };
}

// ============================================================
// UPDATE
// ============================================================

/**
 * 重命名列表
 */
export function dbRenameList(id: string, name: string): void {
  getDb()
    .prepare('UPDATE task_lists SET name = ? WHERE id = ?')
    .run(name, id);
}

/**
 * 修改列表主题色
 */
export function dbSetListColor(
  id: string,
  color: { r: number; g: number; b: number; a: number }
): void {
  getDb().prepare(`
    UPDATE task_lists
    SET    color_r = ?, color_g = ?, color_b = ?, color_a = ?
    WHERE  id = ?
  `).run(color.r, color.g, color.b, color.a, id);
}

// ============================================================
// DELETE
// ============================================================

/**
 * 删除一个自定义列表（级联删除其下所有任务和步骤）
 *
 * ⚠️  此操作不可逆！调用前应在 UI 层弹出确认对话框。
 * 级联删除由数据库外键约束（ON DELETE CASCADE）保证。
 */
export function dbDeleteList(id: string): void {
  getDb()
    .prepare('DELETE FROM task_lists WHERE id = ?')
    .run(id);
}
