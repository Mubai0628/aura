import type { PermissionMode } from '@aura/contracts/generated';

/** Each permission mode's name, in the order the composer's permission menu lists them. */
export const PERMISSION_MODE_NAMES: Record<PermissionMode, string> = {
  full_access: '完全访问',
  read_only: '只读',
  workspace_read_write: '工作区读写',
};
