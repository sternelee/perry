// ============================================================
// 数据库查询层：子任务/步骤（Step）CRUD
//
// Step 是挂载在 Task 下的子级列表项，类似 Microsoft To Do 的"步骤"。
// 每个 Step 归属于一个 Task（外键约束，Task 删除时级联删除）。
// ============================================================

import { getDb } from './database';
import { Step }  from '../types/index';

// ============================================================
// 行类型
// ============================================================

interface StepRow {
  id:         string;
  title:      string;
  is_done:    number;  // 0 | 1
  task_id:    string;
  created_at: number;
}

function rowToStep(row: StepRow): Step {
  return {
    id:        row.id,
    title:     row.title,
    isDone:    row.is_done === 1,
    taskId:    row.task_id,
    createdAt: row.created_at,
  };
}

// ============================================================
// CREATE
// ============================================================

/**
 * 在指定任务下创建一个新步骤
 *
 * @param id     由调用方提供的 UUID
 * @param title  步骤标题（非空）
 * @param taskId 所属任务 ID
 * @returns      新创建的 Step 对象
 */
export function dbCreateStep(id: string, title: string, taskId: string): Step {
  const db  = getDb();
  const now = Date.now();

  db.prepare(`
    INSERT INTO steps (id, title, is_done, task_id, created_at)
    VALUES            (?, ?, 0, ?, ?)
  `).run(id, title, taskId, now);

  return dbGetStep(id)!;
}

// ============================================================
// READ
// ============================================================

/**
 * 按 ID 查询单个步骤
 */
export function dbGetStep(id: string): Step | null {
  const row = getDb()
    .prepare('SELECT * FROM steps WHERE id = ?')
    .get(id) as StepRow | undefined;
  return row ? rowToStep(row) : null;
}

/**
 * 查询指定任务下的所有步骤
 * 排序：未完成在前，已完成在后；同组内按创建时间升序（保持添加顺序）
 */
export function dbGetStepsByTask(taskId: string): Step[] {
  const rows = getDb().prepare(`
    SELECT * FROM steps
    WHERE  task_id = ?
    ORDER  BY is_done ASC, created_at ASC
  `).all(taskId) as StepRow[];
  return rows.map(rowToStep);
}

/**
 * 统计指定任务下步骤的完成进度
 *
 * @returns { total: number, done: number }
 */
export function dbGetStepProgress(taskId: string): { total: number; done: number } {
  const row = getDb().prepare(`
    SELECT
      COUNT(*)                          AS total,
      SUM(CASE WHEN is_done=1 THEN 1 ELSE 0 END) AS done
    FROM steps
    WHERE task_id = ?
  `).get(taskId) as { total: number; done: number };
  return { total: row.total ?? 0, done: row.done ?? 0 };
}

// ============================================================
// UPDATE
// ============================================================

/**
 * 修改步骤标题
 */
export function dbUpdateStepTitle(id: string, title: string): void {
  getDb()
    .prepare('UPDATE steps SET title = ? WHERE id = ?')
    .run(title, id);
}

/**
 * 切换步骤完成状态
 */
export function dbToggleStepDone(id: string, isDone: boolean): void {
  getDb()
    .prepare('UPDATE steps SET is_done = ? WHERE id = ?')
    .run(isDone ? 1 : 0, id);
}

// ============================================================
// DELETE
// ============================================================

/**
 * 删除单个步骤
 */
export function dbDeleteStep(id: string): void {
  getDb()
    .prepare('DELETE FROM steps WHERE id = ?')
    .run(id);
}

/**
 * 删除指定任务下的所有步骤（任务删除前清理，一般通过外键级联）
 */
export function dbDeleteStepsByTask(taskId: string): void {
  getDb()
    .prepare('DELETE FROM steps WHERE task_id = ?')
    .run(taskId);
}
