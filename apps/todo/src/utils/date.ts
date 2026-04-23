// ============================================================
// 日期/时间工具函数
// 遵循单一职责原则：此文件只负责日期格式化与比较，不含 UI 逻辑
// ============================================================

// ============================================================
// 日期字符串工具（YYYY-MM-DD 格式，用于"我的一天"跨天重置）
// ============================================================

/**
 * 获取今天的日期字符串（YYYY-MM-DD，本地时区）
 * 这是"我的一天"功能判断是否跨天的核心依据
 *
 * @example
 * todayStr() // "2026-04-23"
 */
export function todayStr(): string {
  const d = new Date();
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${y}-${m}-${day}`;
}

/**
 * 检查"我的一天"标记是否已过期（跨天检测）
 *
 * 规则：若任务的 myDayDate 不等于今天的日期字符串，则视为过期。
 * 用于启动时批量清除过期的"我的一天"标记（Step 6 中实现）。
 *
 * @param myDayDate 任务的 myDayDate 字段值（YYYY-MM-DD）
 * @returns true = 已过期需要清除；false = 仍为今天
 */
export function isMyDayExpired(myDayDate: string | null): boolean {
  if (!myDayDate) return false;
  return myDayDate !== todayStr();
}

// ============================================================
// 时间戳格式化（用于截止日期显示）
// ============================================================

/**
 * 格式化截止日期时间戳为简短可读字符串
 *
 * 规则：
 * - 今天：显示"今天"
 * - 明天：显示"明天"
 * - 已过期：显示"X月X日（逾期）"
 * - 同年其他日：显示"X月X日 周X"
 * - 跨年：显示"YYYY年X月X日"
 *
 * @param timestamp Unix 时间戳（毫秒）
 * @returns 格式化后的字符串
 */
export function formatDueDate(timestamp: number): string {
  const target = new Date(timestamp);
  const now = new Date();

  // 将两个日期都归零到当天 00:00:00 进行纯日期比较
  const todayMidnight = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const targetMidnight = new Date(target.getFullYear(), target.getMonth(), target.getDate());
  const diffDays = Math.round(
    (targetMidnight.getTime() - todayMidnight.getTime()) / (1000 * 60 * 60 * 24)
  );

  if (diffDays === 0) return '今天';
  if (diffDays === 1) return '明天';
  if (diffDays === -1) return '昨天';

  const month = target.getMonth() + 1;
  const day = target.getDate();
  const weekdays = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'];
  const weekday = weekdays[target.getDay()];

  // 跨年显示完整年份
  if (target.getFullYear() !== now.getFullYear()) {
    return `${target.getFullYear()}年${month}月${day}日`;
  }

  // 过期任务加标记
  if (diffDays < 0) {
    return `${month}月${day}日（逾期）`;
  }

  return `${month}月${day}日 ${weekday}`;
}

/**
 * 判断时间戳是否已过截止日期
 *
 * @param timestamp Unix 时间戳（毫秒）
 * @returns true = 已逾期
 */
export function isOverdue(timestamp: number): boolean {
  const today = new Date();
  today.setHours(23, 59, 59, 999); // 今天结束时刻
  return timestamp < today.getTime() - 1000 * 60 * 60 * 24;
}

/**
 * 判断时间戳是否为今天
 *
 * @param timestamp Unix 时间戳（毫秒）
 */
export function isToday(timestamp: number): boolean {
  const today = new Date();
  const target = new Date(timestamp);
  return (
    target.getFullYear() === today.getFullYear() &&
    target.getMonth() === today.getMonth() &&
    target.getDate() === today.getDate()
  );
}

// ============================================================
// "我的一天"副标题（显示今天的完整日期）
// ============================================================

/**
 * 生成"我的一天"视图的副标题（如"4月23日 星期三"）
 */
export function getMyDaySubtitle(): string {
  const d = new Date();
  return d.toLocaleDateString('zh-CN', {
    month: 'long',
    day: 'numeric',
    weekday: 'long',
  });
}

// ============================================================
// 时间戳与日期字符串互转（用于数据库存储）
// ============================================================

/**
 * 将 YYYY-MM-DD 字符串转为当天 00:00:00 的 Unix 时间戳（毫秒）
 *
 * @example
 * dateStrToTimestamp("2026-04-23") // 1745337600000 (本地时区)
 */
export function dateStrToTimestamp(dateStr: string): number {
  return new Date(dateStr + 'T00:00:00').getTime();
}

/**
 * 将 Unix 时间戳转为 YYYY-MM-DD 格式字符串
 */
export function timestampToDateStr(timestamp: number): string {
  const d = new Date(timestamp);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${y}-${m}-${day}`;
}
