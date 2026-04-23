// ============================================================
// 数据模型类型定义
// 遵循单一职责原则：此文件只负责类型声明，不包含任何逻辑
// ============================================================

/** 任务实体（Task）
 * 对应数据库 tasks 表，是应用的核心数据单元
 */
export interface Task {
  id: string;
  title: string;
  notes: string;           // 备注文本（富文本暂不支持，纯文本）
  isDone: boolean;         // 是否已完成
  isImportant: boolean;    // 是否标星（重要）
  isMyDay: boolean;        // 是否加入"我的一天"
  myDayDate: string | null; // 加入"我的一天"的日期 (YYYY-MM-DD)，跨天重置判断依据
  dueDate: number | null;  // 截止日期 Unix 时间戳 (ms)，null 表示无截止日期
  listId: string;          // 所属自定义列表的 ID
  createdAt: number;       // 创建时间 Unix 时间戳 (ms)
}

/** 子任务/步骤实体（Step）
 * 对应数据库 steps 表，属于某个 Task 的子级项
 */
export interface Step {
  id: string;
  title: string;
  isDone: boolean;    // 是否已完成
  taskId: string;     // 所属任务 ID（外键）
  createdAt: number;  // 创建时间 Unix 时间戳 (ms)
}

/** 自定义任务列表（TaskList）
 * 对应数据库 task_lists 表，用户可自由创建/重命名/删除
 */
export interface TaskList {
  id: string;
  name: string;
  colorR: number;    // 主题色 Red 分量 (0.0 - 1.0)
  colorG: number;    // 主题色 Green 分量 (0.0 - 1.0)
  colorB: number;    // 主题色 Blue 分量 (0.0 - 1.0)
  colorA: number;    // 主题色 Alpha 分量 (0.0 - 1.0)
  createdAt: number; // 创建时间 Unix 时间戳 (ms)
}

// ============================================================
// 智能列表常量
// 智能列表不存储在数据库中，是根据任务属性动态聚合的视图
// ============================================================

/** 智能列表 ID 枚举（使用 "smart:" 前缀与自定义列表区分） */
export const SMART_LIST = {
  MY_DAY:    'smart:my-day',    // 我的一天：isMyDay = true 且 myDayDate = 今天
  IMPORTANT: 'smart:important', // 重要：isImportant = true
  PLANNED:   'smart:planned',   // 计划内：dueDate IS NOT NULL，按时间排序
  ALL:       'smart:all',       // 全部：所有列表的所有任务
} as const;

/** 智能列表 ID 的联合类型 */
export type SmartListId = typeof SMART_LIST[keyof typeof SMART_LIST];

/** 列表选择类型：可以是智能列表 ID 或自定义列表 UUID */
export type ListSelection = SmartListId | string;

/** 判断一个 ListSelection 是否为智能列表 */
export function isSmartList(id: ListSelection): id is SmartListId {
  return id.startsWith('smart:');
}

// ============================================================
// UI 相关类型
// ============================================================

/** 主题模式 */
export type ThemeMode = 'light' | 'dark';

/** 布局模式（桌面三栏 vs 移动单栏） */
export type LayoutMode = 'desktop' | 'mobile';

/** 智能列表的元数据（用于侧边栏渲染） */
export interface SmartListMeta {
  id: SmartListId;
  label: string;       // 显示名称
  icon: string;        // SF Symbol 名称（macOS/iOS）
  iconFallback: string; // 文本 fallback（Linux/Windows 无 SF Symbol）
}

/** 预定义的智能列表元数据 */
export const SMART_LIST_META: SmartListMeta[] = [
  {
    id: SMART_LIST.MY_DAY,
    label: '我的一天',
    icon: 'sun.max',
    iconFallback: '☀',
  },
  {
    id: SMART_LIST.IMPORTANT,
    label: '重要',
    icon: 'star',
    iconFallback: '★',
  },
  {
    id: SMART_LIST.PLANNED,
    label: '计划内',
    icon: 'calendar',
    iconFallback: '📅',
  },
  {
    id: SMART_LIST.ALL,
    label: '全部',
    icon: 'tray.full',
    iconFallback: '≡',
  },
];
