import type { MemoryMaintenanceRun } from '@aura/contracts/generated';

const STATE: Record<MemoryMaintenanceRun['state'], string> = {
  queued: '等待',
  running: '进行中',
  completed: '已完成',
  skipped: '已跳过',
  failed: '失败',
  interrupted: '已中断',
};

/** When the run ended (or began, while it runs), in the user's local time. */
export function runTime(run: MemoryMaintenanceRun): string {
  const at = new Date(run.finished_at ?? run.started_at);
  return Number.isNaN(at.getTime())
    ? (run.finished_at ?? run.started_at)
    : at.toLocaleString('zh-CN', {
        month: 'numeric',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        hour12: false,
      });
}

/** 已完成：新增 2 条 */
export const runSummary = (run: MemoryMaintenanceRun): string =>
  `${STATE[run.state]}${run.reason ? `：${run.reason}` : ''}`;

/** 会话 ses_… · 输入 120 / 输出 30 token */
export const runDetail = (run: MemoryMaintenanceRun): string =>
  `会话 ${run.session_id} · 输入 ${run.input_tokens} / 输出 ${run.output_tokens} token${
    run.usage_complete ? '' : '（用量未确认）'
  }`;
