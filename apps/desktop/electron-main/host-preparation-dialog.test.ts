import { expect, test } from 'vitest';

import { hostPreparationDialog } from './host-preparation-dialog.js';

// Main's confirmation for a Windows change names the action, what changes and until when the plan
// holds, in words; no English title, UTC time or security jargon.

test('the confirmation says what the user approves, in words', () => {
  const now = new Date(2026, 10, 10, 9, 0);
  const dialog = hostPreparationDialog(
    {
      action: 'prepare',
      add_all_application_packages_ace: true,
      add_all_restricted_application_packages_ace: false,
      expires_at: new Date(2026, 10, 10, 16, 15).toISOString(),
      update_null_device_security: true,
    },
    now,
  );
  expect(dialog.title).toBe('Aura 兼容环境');
  expect(dialog.message).toBe('配置兼容环境');
  expect(dialog.detail).toBe(
    [
      '会修改 Windows 里几处系统位置的权限，让 Aura 沙箱里的程序能正常运行。',
      '这份方案在 16:15 前有效',
      '• 让 Windows 沙箱里的程序能访问 Aura 需要的系统位置（会修改这些位置的权限）',
      '• 让沙箱里的程序能使用空设备 NUL（会修改它的安全设置）',
    ].join('\n'),
  );
  expect(`${dialog.title}${dialog.message}${dialog.detail}`).not.toMatch(
    /Host Preparation|ACE|APPLICATION PACKAGES|T\d\d:\d\d|Z$/u,
  );
});
