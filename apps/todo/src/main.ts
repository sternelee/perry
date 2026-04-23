// ============================================================
// Perry Todo — 应用入口（main.ts）
//
// 职责（单一原则：仅负责启动流程编排）：
//   1. 初始化数据库（创建表、写入种子数据）
//   2. 检测运行时环境（设备类型、深色模式）
//   3. 构建对应平台的布局（桌面三栏 or 移动 TabBar）
//   4. 启动 Perry App 主循环
//   5. （可选）注册全局快捷键
//
// 所有业务逻辑不在此文件实现，通过导入各模块完成。
// ============================================================

import { App, addKeyboardShortcut } from 'perry/ui';
import { isDarkMode, getDeviceIdiom } from 'perry/system';

import { getDb }                             from './db/database';
import { appState, setDarkMode, selectList } from './state/app-state';
import { buildDesktopLayout }                from './views/desktop/layout';
import { buildMobileLayout, mobileNavStacks } from './views/mobile/layout';
import { loadAllLists }                      from './services/list-service';
import { loadCurrentTasks, resetExpiredMyDay } from './services/task-service';
import { SMART_LIST }                        from './types/index';

// ============================================================
// 步骤 1：数据库初始化
// getDb() 是幂等的单例工厂：首次调用时建表、写种子数据，
// 后续调用直接返回缓存连接。
// ============================================================
getDb();

// ── 步骤 2 新增：加载初始数据 ─────────────────────────────
// 跨天重置"我的一天"（若昨天忘记关闭应用，今天启动时自动清零）
resetExpiredMyDay();

// 加载所有自定义列表到内存缓存（侧边栏需要）
loadAllLists();

// 加载当前视图（默认"我的一天"）的任务
loadCurrentTasks();

// ============================================================
// 步骤 2：检测运行时环境
// ============================================================

// 检测系统深色模式（macOS/iOS/Windows 跟随系统）
appState.isDark = isDarkMode();

// 检测设备类型，决定布局策略
// getDeviceIdiom() 返回: "mac" | "phone" | "pad" | "tv"
const idiom = getDeviceIdiom();
appState.layout = (idiom === 'phone') ? 'mobile' : 'desktop';

// ============================================================
// 步骤 3：构建布局
// ============================================================

let body: any;

if (appState.layout === 'mobile') {
  // 移动端：底部 TabBar + NavStack 组合
  const mobilePanels = buildMobileLayout();
  // 将 NavStack 句柄导出到移动导航辅助模块
  mobileNavStacks.push(...mobilePanels.navStacks);
  body = mobilePanels.root;
} else {
  // 桌面端：三栏 HStack（侧边栏 + 任务列表 + 详情面板）
  const desktopPanels = buildDesktopLayout();
  body = desktopPanels.root;
}

// ============================================================
// 步骤 4：启动 Perry App 主循环
// ============================================================

App({
  title: 'Perry Todo',
  // 桌面默认窗口尺寸（1100×700 是 Microsoft To Do 桌面版的近似尺寸）
  // 移动端 width/height 被 Perry 忽略，由系统决定全屏
  width:  1100,
  height: 700,
  body,
});

// ============================================================
// 步骤 5：注册全局快捷键（macOS Cmd / Windows Ctrl）
// ============================================================

// Cmd/Ctrl + D：切换深色/浅色模式（开发调试用）
addKeyboardShortcut('d', 1, () => {
  setDarkMode(!appState.isDark);
});

// Cmd/Ctrl + N：快速新建任务（Step 4 中绑定到输入框）
addKeyboardShortcut('n', 1, () => {
  // Step 4 实现：聚焦快速输入框
  // textfieldFocus(quickInputField);
});

// Cmd/Ctrl + W：关闭详情面板
addKeyboardShortcut('w', 1, () => {
  if (appState.isDetailOpen) {
    // Step 5 实现：关闭详情面板
    // selectTask(null);
  }
});
