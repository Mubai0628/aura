import type { RecoveryNotice } from '@aura/contracts/generated';

// Bootstrap must not turn a reported, repairable configuration fault into a Core restart.
// Only the capabilities named by the authoritative notice are deferred. Workspace/session
// data and Core health still synchronize; arbitrary projection/transport errors still fail.
export function recoveryProjectionPlan(notice?: RecoveryNotice) {
  const unavailable = new Set(notice?.unavailable_capabilities ?? []);
  return {
    workspaceFiles: !unavailable.has('tool_policy'),
    tools: !unavailable.has('tool_policy'),
    models: !unavailable.has('model_authority'),
  };
}
