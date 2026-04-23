// ============================================================
// 数据库层：连接管理与 Schema 初始化
// 职责：
//   - 管理 SQLite 连接（单例）
//   - 在首次启动时创建所有表和索引
//   - 写入种子数据（默认"任务"列表）
// ============================================================

import Database from 'better-sqlite3';
import { join } from 'path';
import { existsSync, mkdirSync } from 'fs';

// ============================================================
// 连接管理
// ============================================================

/** 单例连接引用，避免在同一进程内重复打开 DB 文件 */
let _db: ReturnType<typeof Database> | null = null;

/**
 * 获取数据库文件的存储路径（跨平台）
 *
 * - macOS:   ~/Library/Application Support/PerryTodo/todo.db （通过 HOME）
 * - Linux:   ~/.todo-app/todo.db
 * - Windows: %USERPROFILE%\.todo-app\todo.db
 * - iOS:     Perry 会将相对路径自动解析到 Documents/ 目录
 */
function getDbPath(): string {
  const homeDir = process.env.HOME || process.env.USERPROFILE || '.';
  const appDataDir = join(homeDir, '.todo-app');

  // 确保数据目录存在（Perry 的 mkdirSync 跨平台兼容）
  if (!existsSync(appDataDir)) {
    mkdirSync(appDataDir, { recursive: true });
  }

  return join(appDataDir, 'todo.db');
}

/**
 * 获取数据库连接（单例）
 *
 * 首次调用时：
 *   1. 打开或创建数据库文件
 *   2. 配置 PRAGMA（WAL 模式 + 外键约束）
 *   3. 初始化表结构
 *   4. 写入种子数据
 */
export function getDb(): ReturnType<typeof Database> {
  if (_db) return _db;

  const dbPath = getDbPath();
  _db = new Database(dbPath);

  // WAL 模式：提升并发写入性能，减少读写锁争用
  _db.exec('PRAGMA journal_mode = WAL;');

  // 启用外键约束（better-sqlite3 默认关闭）
  // 确保删除列表时级联删除其下的任务，删除任务时级联删除步骤
  _db.exec('PRAGMA foreign_keys = ON;');

  initSchema(_db);
  seedDefaultData(_db);

  return _db;
}

/**
 * 关闭数据库连接（应用退出时调用）
 * Perry 的进程模型下通常不需要手动关闭，
 * 但提供此接口供测试和显式清理使用。
 */
export function closeDb(): void {
  if (_db) {
    _db.close();
    _db = null;
  }
}

// ============================================================
// Schema 初始化
// ============================================================

/**
 * 创建所有表和索引（幂等：IF NOT EXISTS 保证重复调用安全）
 *
 * 表结构设计原则：
 * - 使用 TEXT 存储 UUID，避免 INTEGER 主键在分布式场景下的冲突
 * - 布尔值用 INTEGER (0/1) 存储（SQLite 无原生布尔类型）
 * - 时间戳使用 INTEGER 存储毫秒级 Unix epoch（与 Date.now() 一致）
 * - REAL 存储颜色分量（0.0 - 1.0），直接对应 Perry UI 的 RGBA API
 */
function initSchema(db: ReturnType<typeof Database>): void {
  db.exec(`
    -- ── 自定义任务列表表 ──────────────────────────────────
    CREATE TABLE IF NOT EXISTS task_lists (
      id         TEXT    PRIMARY KEY,
      name       TEXT    NOT NULL,
      color_r    REAL    NOT NULL DEFAULT 0.0,   -- 主题色 Red   (0.0-1.0)
      color_g    REAL    NOT NULL DEFAULT 0.478, -- 主题色 Green (0.0-1.0)
      color_b    REAL    NOT NULL DEFAULT 0.831, -- 主题色 Blue  (0.0-1.0)
      color_a    REAL    NOT NULL DEFAULT 1.0,   -- 主题色 Alpha (0.0-1.0)
      created_at INTEGER NOT NULL
    );

    -- ── 任务表 ────────────────────────────────────────────
    CREATE TABLE IF NOT EXISTS tasks (
      id           TEXT    PRIMARY KEY,
      title        TEXT    NOT NULL,
      notes        TEXT    NOT NULL DEFAULT '',
      is_done      INTEGER NOT NULL DEFAULT 0,       -- 0: 未完成, 1: 已完成
      is_important INTEGER NOT NULL DEFAULT 0,       -- 0: 普通, 1: 星标重要
      is_my_day    INTEGER NOT NULL DEFAULT 0,       -- 0: 不在今天, 1: 加入我的一天
      my_day_date  TEXT,                             -- 加入时的日期 (YYYY-MM-DD)，跨天清零用
      due_date     INTEGER,                          -- 截止日期时间戳 (ms)，NULL 表示无
      list_id      TEXT    NOT NULL,                 -- 所属列表 ID（外键）
      created_at   INTEGER NOT NULL,
      FOREIGN KEY (list_id) REFERENCES task_lists(id) ON DELETE CASCADE
    );

    -- ── 子任务/步骤表 ──────────────────────────────────────
    CREATE TABLE IF NOT EXISTS steps (
      id         TEXT    PRIMARY KEY,
      title      TEXT    NOT NULL,
      is_done    INTEGER NOT NULL DEFAULT 0,  -- 0: 未完成, 1: 已完成
      task_id    TEXT    NOT NULL,            -- 所属任务 ID（外键）
      created_at INTEGER NOT NULL,
      FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
    );

    -- ── 索引：加速常见查询 ────────────────────────────────
    -- 按列表 ID 查询任务（最高频查询）
    CREATE INDEX IF NOT EXISTS idx_tasks_list_id
      ON tasks(list_id);

    -- 按截止日期查询（"计划内"智能列表）
    CREATE INDEX IF NOT EXISTS idx_tasks_due_date
      ON tasks(due_date)
      WHERE due_date IS NOT NULL;

    -- "我的一天"索引
    CREATE INDEX IF NOT EXISTS idx_tasks_my_day
      ON tasks(is_my_day, my_day_date)
      WHERE is_my_day = 1;

    -- "重要"索引
    CREATE INDEX IF NOT EXISTS idx_tasks_important
      ON tasks(is_important)
      WHERE is_important = 1;

    -- 按步骤所属任务查询
    CREATE INDEX IF NOT EXISTS idx_steps_task_id
      ON steps(task_id);
  `);
}

// ============================================================
// 种子数据
// ============================================================

/**
 * 写入初始数据（仅在数据库为空时执行，保证幂等）
 *
 * 默认创建一个"任务"列表，使用 Microsoft Blue 主题色。
 * 这是类似 Microsoft To Do 默认"任务"列表的设计。
 */
function seedDefaultData(db: ReturnType<typeof Database>): void {
  const row = db.prepare(
    'SELECT COUNT(*) as count FROM task_lists'
  ).get() as { count: number };

  // 只在没有任何列表时写入种子数据
  if (row.count === 0) {
    const now = Date.now();

    // 默认"任务"列表（Microsoft Blue）
    db.prepare(`
      INSERT INTO task_lists (id, name, color_r, color_g, color_b, color_a, created_at)
      VALUES (?, ?, ?, ?, ?, ?, ?)
    `).run('list:tasks', '任务', 0.0, 0.478, 0.831, 1.0, now);

    // 可选：写入一条欢迎任务，让新用户看到应用效果
    db.prepare(`
      INSERT INTO tasks (id, title, notes, is_done, is_important, is_my_day,
                         my_day_date, due_date, list_id, created_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `).run(
      'task:welcome',
      '欢迎使用 Perry Todo！点击任务查看详情 ✓',
      '这是一个基于 Perry 原生编译器构建的全平台 TODO 应用。',
      0, 0, 1,
      new Date().toISOString().slice(0, 10), // 今天日期
      null,
      'list:tasks',
      now
    );
  }
}
