import { expect, test } from 'vitest';

import { HOST_ACTION_TITLE, hostPlanLines, hostPlanValidUntil } from './host-preparation-text.js';

// The compatibility environment is named in words, its plan says what it changes in Windows,
// and how long the plan holds is a local time.

test('each action and each change of the plan is said in words', () => {
  expect(HOST_ACTION_TITLE).toEqual({
    prepare: '配置兼容环境',
    repair: '修复兼容环境',
    remove: '移除 Aura 配置',
  });
  expect(
    hostPlanLines({
      action: 'prepare',
      add_all_application_packages_ace: true,
      add_all_restricted_application_packages_ace: true,
      update_null_device_security: true,
    }),
  ).toEqual([
    '让 Windows 沙箱里的程序能访问 Aura 需要的系统位置（会修改这些位置的权限）',
    '让受限沙箱里的程序也能访问这些位置（会修改这些位置的权限）',
    '让沙箱里的程序能使用空设备 NUL（会修改它的安全设置）',
  ]);
  // For removal each flag is a change Aura made earlier that the plan takes back.
  expect(
    hostPlanLines({
      action: 'remove',
      add_all_application_packages_ace: true,
      add_all_restricted_application_packages_ace: false,
      update_null_device_security: true,
    }),
  ).toEqual([
    '撤回 Aura 给 Windows 沙箱程序加的系统位置访问权限',
    '把空设备 NUL 的安全设置恢复成 Aura 修改之前的样子',
  ]);
  const now = new Date(2026, 10, 10, 9, 0);
  expect(hostPlanValidUntil(new Date(2026, 10, 10, 16, 15).toISOString(), now)).toBe(
    '这份方案在 16:15 前有效',
  );
});
