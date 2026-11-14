import type { HostPreparationAction } from '@aura/contracts/generated';
import { localTime } from './local-time.js';

/** The compatibility environment's three actions, by the names of their buttons. */
export const HOST_ACTION_TITLE: Record<HostPreparationAction, string> = {
  prepare: '配置兼容环境',
  repair: '修复兼容环境',
  remove: '移除 Aura 配置',
};

/** What each action does to Windows, in one sentence. */
export const HOST_ACTION_SUMMARY: Record<HostPreparationAction, string> = {
  prepare: '会修改 Windows 里几处系统位置的权限，让 Aura 沙箱里的程序能正常运行。',
  repair: '会把 Aura 改过的系统权限恢复到需要的状态。',
  remove: '只撤回 Aura 自己改过的系统权限，其他设置不动。',
};

/**
 * The changes one plan makes. For removal each flag is a change Aura made earlier that the plan
 * takes back; otherwise it is a change the plan makes.
 */
export interface HostPlanChanges {
  action: HostPreparationAction;
  add_all_application_packages_ace: boolean;
  add_all_restricted_application_packages_ace: boolean;
  update_null_device_security: boolean;
}

const MAKE = [
  '让 Windows 沙箱里的程序能访问 Aura 需要的系统位置（会修改这些位置的权限）',
  '让受限沙箱里的程序也能访问这些位置（会修改这些位置的权限）',
  '让沙箱里的程序能使用空设备 NUL（会修改它的安全设置）',
] as const;
const TAKE_BACK = [
  '撤回 Aura 给 Windows 沙箱程序加的系统位置访问权限',
  '撤回 Aura 给受限沙箱程序加的访问权限',
  '把空设备 NUL 的安全设置恢复成 Aura 修改之前的样子',
] as const;

/** Each change of the plan, in words. */
export function hostPlanLines(plan: HostPlanChanges): string[] {
  const words = plan.action === 'remove' ? TAKE_BACK : MAKE;
  return [
    plan.add_all_application_packages_ace ? words[0] : undefined,
    plan.add_all_restricted_application_packages_ace ? words[1] : undefined,
    plan.update_null_device_security ? words[2] : undefined,
  ].filter((line): line is (typeof words)[number] => line !== undefined);
}

/** Until when a prepared plan holds, as a local time. */
export const hostPlanValidUntil = (expiresAt: string, now = new Date()): string =>
  `这份方案在 ${localTime(expiresAt, now)} 前有效`;
