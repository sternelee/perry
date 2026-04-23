// ============================================================
// 数据库查询层：任务（Task）CRUD
//
// 设计原则：
//   - 所有查询使用 Prepared Statement（防 SQL 注入，better-sqlite3 自动缓存）
//   - 行数据与 TypeScript 接口之间通过 rowToTask() 做一次集中转换
//   - 此文件不包含业务规则，只负责"数据库行 ↔ TypeScript 对象"的 I/O
//   - 调用方负责提供合法参数，此层不做额外校验
// ============================================================

import { getDb }          from './database';
import { Task }           from '../types/index';
import { todayStr }       from '../utils/date';

// ============================================================
// 行类型（SQLite 返回的原始 JS 对象）
// ============================================================

/** better-sqlite3 返回的任务行（字段名与数据库列名一致）*/
interface TaskRow {
  id:           string;
  title:        string;
  notes:        string;
  is_done:      number;   // 0 | 1
  is_important: number;   // 0 | 1
  is_my_day:    number;   // 0 | 1
  my_day_date:  string | null;
  due_date:     number | null;
  list_id:      string;
  created_at:   number;
}

/** 将数据库行映射为 TypeScript Task 对象 */
function rowToTask(row: TaskRow): Task {
  return {
    id:          row.id,
    title:       row.title,
    notes:       row.notes,
    isDone:      row.is_done === 1,
    isImportant: row.is_important === 1,
    isMyDay:     row.is_my_day === 1,
    myDayDate:   row.my_day_date,
    dueDate:     row.due_date,
    listId:      row.list_id,
    createdAt:   row.created_at,
  };
}

// ============================================================
// CREATE
// ============================================================

/**
 * 在指定列表中创建一条新任务
 *
 * @param id      调用方提供的 UUID（由 generateId() 生成）
 * @param title   任务标题（非空）
 * @param listId  所属列表 ID
 * @param options 可选附加属性
 * @returns       插入成功后立即查出并返回完整 Task 对象
 */
export function dbCreateTask(
  id: string,
  title: string,
  listId: string,
  options: {
    notes?:       string;
    isImportant?: boolean;
    isMyDay?:     boolean;
    dueDate?:     number | null;
  } = {}
): Task {
  const db  = getDb();
  const now = Date.now();

  // 若创建时就加入"我的一天"，记录当天日期以支持跨天重置
  const myDayDate = options.isMyDay ? todayStr() : null;

  db.prepare(`
    INSERT INTO tasks
      (id, title, notes, is_done, is_important, is_my_day,
       my_day_date, due_date, list_id, created_at)
    VALUES
      (?, ?, ?, 0, ?, ?, ?, ?, ?, ?)
  `).run(
    id,
    title,
    options.notes       ?? '',
    options.isImportant ? 1 : 0,
    options.isMyDay     ? 1 : 0,
    myDayDate,
    options.dueDate     ?? null,
    listId,
    now
  );

  // 立即回查确保返回数据与实际存储一致
  return dbGetTask(id)!;
}

// ============================================================
// READ
// ============================================================

/**
 * 按 ID 查询单条任务（不存在时返回 null）
 */
export function dbGetTask(id: string): Task | null {
  const row = getDb()
    .prepare('SELECT * FROM tasks WHERE id = ?')
    .get(id) as TaskRow | undefined;
  return row ? rowToTask(row) : null;
}

/**
 * 查询指定自定义列表下的所有任务
 * 排序规则：未完成在前，已完成在后；同组内按创建时间倒序
 */
export function dbGetTasksByList(listId: string): Task[] {
  const rows = getDb().prepare(`
    SELECT * FROM tasks
    WHERE  list_id = ?
    ORDER  BY is_done ASC, created_at DESC
  `).all(listId) as TaskRow[];
  return rows.map(rowToTask);
}

/**
 * 查询"我的一天"任务
 * 条件：is_my_day = 1 AND my_day_date = 今天（过期标记不显示）
 */
export function dbGetMyDayTasks(): Task[] {
  const rows = getDb().prepare(`
    SELECT * FROM tasks
    WHERE  is_my_day = 1
    AND    my_day_date = ?
    ORDER  BY is_done ASC, created_at DESC
  `).all(todayStr()) as TaskRow[];
  return rows.map(rowToTask);
}

/**
 * 查询所有被标为"重要"的任务
 * 按完成状态排序，同组内按创建时间倒序
 */
export function dbGetImportantTasks(): Task[] {
  const rows = getDb().prepare(`
    SELECT * FROM tasks
    WHERE  is_important = 1
    ORDER  BY is_done ASC, created_at DESC
  `).all() as TaskRow[];
  return rows.map(rowToTask);
}

/**
 * 查询"计划内"任务（有截止日期的任务）
 * 按截止日期升序排列（最近到期的排在最前）
 */
export function dbGetPlannedTasks(): Task[] {
  const rows = getDb().prepare(`
    SELECT * FROM tasks
    WHERE  due_date IS NOT NULL
    ORDER  BY is_done ASC, due_date ASC
  `).all() as TaskRow[];
  return rows.map(rowToTask);
}

/**
 * 查询全部任务（跨所有列表）
 * 按列表分组，列表内按完成状态 + 创建时间排序
 */
export function dbGetAllTasks(): Task[] {
  const rows = getDb().prepare(`
    SELECT t.* FROM tasks t
    JOIN   task_lists l ON t.list_id = l.id
    ORDER  BY l.created_at ASC, t.is_done ASC, t.created_at DESC
  `).all() as TaskRow[];
  return rows.map(rowToTask);
}

// ============================================================
// UPDATE
// ============================================================

/**
 * 修改任务标题
 */
export function dbUpdateTaskTitle(id: string, title: string): void {
  getDb()
    .prepare('UPDATE tasks SET title = ? WHERE id = ?')
    .run(title, id);
}

/**
 * 修改任务备注
 */
export function dbUpdateTaskNotes(id: string, notes: string): void {
  getDb()
    .prepare('UPDATE tasks SET notes = ? WHERE id = ?')
    .run(notes, id);
}

/**
 * 切换任务完成状态
 */
export function dbToggleTaskDone(id: string, isDone: boolean): void {
  getDb()
    .prepare('UPDATE tasks SET is_done = ? WHERE id = ?')
    .run(isDone ? 1 : 0, id);
}

/**
 * 切换任务"重要"（星标）状态
 */
export function dbToggleTaskImportant(id: string, isImportant: boolean): void {
  getDb()
    .prepare('UPDATE tasks SET is_important = ? WHERE id = ?')
    .run(isImportant ? 1 : 0, id);
}

/**
 * 切换"加入/移除我的一天"
 *
 * 加入时记录今天的日期字符串，用于次日启动时的跨天重置检测。
 */
export function dbToggleTaskMyDay(id: string, isMyDay: boolean): void {
  getDb().prepare(`
    UPDATE tasks
    SET    is_my_day = ?, my_day_date = ?
    WHERE  id = ?
  `).run(isMyDay ? 1 : 0, isMyDay ? todayStr() : null, id);
}

/**
 * 设置或清除截止日期
 *
 * @param dueDate Unix 时间戳（毫秒），null 表示清除
 */
export function dbSetTaskDueDate(id: string, dueDate: number | null): void {
  getDb()
    .prepare('UPDATE tasks SET due_date = ? WHERE id = ?')
    .run(dueDate, id);
}

/**
 * 将任务移动到另一个列表
 */
export function dbMoveTask(id: string, newListId: string): void {
  getDb()
    .prepare('UPDATE tasks SET list_id = ? WHERE id = ?')
    .run(newListId, id);
}

// ============================================================
// DELETE
// ============================================================

/**
 * 删除单条任务（级联删除其下的所有步骤，由外键约束保证）
 */
export function dbDeleteTask(id: string): void {
  getDb()
    .prepare('DELETE FROM tasks WHERE id = ?')
    .run(id);
}

/**
 * 删除指定列表下所有已完成的任务（批量清理）
 */
export function dbDeleteCompletedInList(listId: string): void {
  getDb().prepare(`
    DELETE FROM tasks
    WHERE  list_id = ? AND is_done = 1
  `).run(listId);
}

// ============================================================
// 跨天重置（"我的一天"）
// ============================================================

/**
 * 将所有过期的"我的一天"标记清零
 *
 * 规则：my_day_date != 今天 的任务，将 is_my_day 和 my_day_date 置空。
 * 调用时机：应用启动时（main.ts → resetExpiredMyDay()）。
 * 注意：此操作只移除"我的一天"标记，不删除任务本身。
 *
 * @returns 被重置的任务数量
 */
export function dbResetExpiredMyDay(): number {
  const result = getDb().prepare(`
    UPDATE tasks
    SET    is_my_day = 0, my_day_date = NULL
    WHERE  is_my_day = 1
    AND    my_day_date IS NOT NULL
    AND    my_day_date != ?
  `).run(todayStr()) as { changes: number };
  return result.changes;
}
