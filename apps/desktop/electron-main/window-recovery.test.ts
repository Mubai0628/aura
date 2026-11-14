// @vitest-environment node
import { expect, test } from 'vitest';
import {
  pageLoadFailureText,
  rendererGoneText,
  RendererCrashes,
  RENDERER_CRASH_LOOP,
  startFailureText,
} from './window-recovery.js';

// A window that went away says why, what it means and what to do; a loop of them stops.
test('a page that ended says why and that Core went on', () => {
  expect(rendererGoneText('oom')).toBe(
    'Aura 界面意外退出（内存不足），已自动重新打开；Core 和正在运行的任务不受影响，未发送的输入已尽量保留。反复出现时请关闭不用的会话后重启 Aura。',
  );
  expect(rendererGoneText('crashed')).toContain('（程序出错）');
  expect(rendererGoneText('something-new')).toContain('（原因不明）');
});

test('the third end within two minutes stops reopening', () => {
  const crashes = new RendererCrashes();
  expect(crashes.record(0)).toBe('reopen');
  expect(crashes.record(60_000)).toBe('reopen');
  // The first two are older than two minutes by now.
  expect(crashes.record(200_000)).toBe('reopen');
  expect(crashes.record(230_000)).toBe('reopen');
  expect(crashes.record(250_000)).toBe('stop');
  expect(RENDERER_CRASH_LOOP).toBe(
    'Aura 界面反复意外退出，已停止自动重新打开；Core 和正在运行的任务不受影响。请退出 Aura 后重新打开，仍然如此请重新运行安装程序修复。',
  );
});

test('a page that did not load says so, for the installed files and for a development run', () => {
  expect(pageLoadFailureText(undefined)).toBe(
    'Aura 界面没有加载出来（界面文件缺失或损坏），Aura 现在不能使用；请重新运行安装程序修复，或在启动器中回到上一版本。',
  );
  expect(pageLoadFailureText('http://127.0.0.1:5175')).toBe(
    'Aura 界面没有加载出来（开发服务器 http://127.0.0.1:5175 没有响应），Aura 现在不能使用；请先启动开发服务器再打开。',
  );
});

test('a release that failed its check, and a start that threw, read as words', () => {
  expect(startFailureText('release')).toBe(
    'Aura 的安装文件不完整或被改动过（校验没有通过），Aura Core 没有启动，所有功能暂不可用；请重新运行安装程序修复，或在启动器中回到上一版本。',
  );
  expect(startFailureText('supervisor')).toBe(
    'Aura Core 没能启动（启动过程出了意外的错误，详情记在 Main 日志里），所有功能暂不可用；请重启 Aura，仍然如此请重新运行安装程序修复。',
  );
});
