// platform.ts — compile-time platform constants for Perry UI
//
// `__platform__` is a compile-time integer constant injected by the perry compiler.
// The value is determined at compile time (not runtime) based on the --target flag:
//   0 = macOS  |  1 = iOS  |  2 = Android  |  3 = Windows  |  4 = Linux
//
// Because the value is a compile-time constant, Cranelift constant-folds all
// comparisons and eliminates dead branches — true DCE with zero runtime cost.

declare const __platform__: number;

export const Platform = {
  MACOS:   0,
  IOS:     1,
  ANDROID: 2,
  WINDOWS: 3,
  LINUX:   4,
} as const;

// Individual platform booleans — Cranelift folds these to true/false at compile time
export const isMac     = __platform__ === Platform.MACOS;
export const isIOS     = __platform__ === Platform.IOS;
export const isAndroid = __platform__ === Platform.ANDROID;
export const isWindows = __platform__ === Platform.WINDOWS;
export const isLinux   = __platform__ === Platform.LINUX;

// Convenience groupings
export const isDesktop = __platform__ === Platform.MACOS
  || __platform__ === Platform.WINDOWS
  || __platform__ === Platform.LINUX;

export const isMobile  = __platform__ === Platform.IOS
  || __platform__ === Platform.ANDROID;
