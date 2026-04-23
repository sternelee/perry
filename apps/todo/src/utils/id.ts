// ============================================================
// 唯一 ID 生成工具
// 使用 crypto.randomBytes 生成 UUID v4 格式的 ID
// ============================================================

import { randomBytes } from 'crypto';

/**
 * 生成一个 UUID v4 格式的唯一 ID
 *
 * 格式: xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx
 * 基于 crypto.randomBytes，加密安全级别的随机性
 *
 * @example
 * const taskId = generateId();  // "f47ac10b-58cc-4372-a567-0e02b2c3d479"
 */
export function generateId(): string {
  const bytes = randomBytes(16);

  // 设置 UUID version 4 标志位（第 6 字节高 4 位 = 0100）
  bytes[6] = (bytes[6] & 0x0f) | 0x40;

  // 设置 UUID variant 标志位（第 8 字节高 2 位 = 10）
  bytes[8] = (bytes[8] & 0x3f) | 0x80;

  const h = bytes.toString('hex');
  return [
    h.slice(0, 8),
    h.slice(8, 12),
    h.slice(12, 16),
    h.slice(16, 20),
    h.slice(20),
  ].join('-');
}

/**
 * 生成带前缀的 ID（用于调试时快速识别实体类型）
 *
 * @example
 * generatePrefixedId('task')  // "task_f47ac10b58cc4372a5670e02b2c3d479"
 */
export function generatePrefixedId(prefix: string): string {
  return `${prefix}_${randomBytes(16).toString('hex')}`;
}
