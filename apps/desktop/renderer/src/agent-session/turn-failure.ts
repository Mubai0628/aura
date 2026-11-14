import { userMessage } from './user-message';

/**
 * Why a turn ended, for Aura's own reasons, in sentences about the task. They speak of the task,
 * so they are read for a turn only, never for a command refused with the same code.
 */
const turnMessages: Record<string, string> = {
  'agent.cancelled': '这一轮已停止，过程输出仍可阅读；需要时在输入框说明下一步。',
  'agent.goal_budget_exhausted':
    'Goal 的预算已经用完，任务已停下，过程输出仍可阅读。可以在近期活动里调高预算后继续，或在输入框说明下一步。',
  'agent.tool_outcome_unknown':
    '有工具调用已经发出，但没有拿到结果，无法确认是否已经执行，所以没有自动重做。任务已暂停；请先检查相关文件或进程的实际状态，再在输入框说明下一步。',
  'agent.run_panicked':
    '这一步执行时 Aura 内部出错（已记入 Core 日志），任务已暂停，没有重放未完成的操作。过程输出仍可阅读；在输入框发“继续”即可接着做。',
  'model.profile_missing':
    '这个会话用的模型已不存在或已停用，这一步没有发出。任务已暂停；请在输入框上方换一个模型，再发“继续”。',
  'model.endpoint_snapshot_mismatch':
    '这一步准备期间，模型的连接或密钥被改过，为免用错设置没有发出。任务已暂停；在输入框发“继续”会按新设置接着做。',
  'model.credential_changed':
    '这一步准备期间，模型的连接或密钥被改过，为免用错设置没有发出。任务已暂停；在输入框发“继续”会按新设置接着做。',
  'model.event_channel_closed':
    '与模型服务的内部通道意外关闭，这一步没有完成。任务已暂停；在输入框发“继续”即可接着做。',
  'agent.store_refused':
    'Aura 的数据库正忙，这一步没有开始。任务已暂停，过程输出仍可阅读；稍后在输入框发“继续”即可接着做。',
  'agent.store_unknown':
    'Aura 的数据库没有及时答复，无法确认这一步是否已经写入，所以没有自动重做。任务已暂停；请刷新后核对，再在输入框发“继续”。',
  'agent.store_failed':
    'Aura 的数据库读写失败（详情在 Core 日志里），任务已暂停，过程输出仍可阅读；重启 Aura 后在输入框发“继续”即可接着做。',
  'agent.store_unavailable':
    'Aura 的数据库读写失败（详情在 Core 日志里），任务已暂停，过程输出仍可阅读；重启 Aura 后在输入框发“继续”即可接着做。',
  'context.compaction_no_progress':
    '压缩较早的对话已经无法再让请求变小，这一步没有发出。任务已暂停；可以清理较早的工具结果，或换一个上下文更大的模型，然后在输入框发“继续”。',
  'context.budget_overflow':
    '压缩较早的对话已经无法再让请求变小，这一步没有发出。任务已暂停；可以清理较早的工具结果，或换一个上下文更大的模型，然后在输入框发“继续”。',
};

/** What a turn that ended on a reason with no sentence of its own says. */
export const TURN_FALLBACK =
  '这一步没有完成（原因已记入 Core 日志），任务已暂停，没有重放未完成的操作。过程输出仍可阅读；在输入框发“继续”即可接着做。';

/**
 * What the user reads about why a turn ended, from the failure Core stored for it or its stop
 * reason: a model service's failure and every code with a sentence of its own say what happened
 * and what to do next.
 */
export function turnFailureMessage(text: string): string {
  // Core quotes filenames with 「」. They are user content, even when a name resembles a code.
  const diagnostics = text.replace(/「[^」]*」/gu, '');
  if (!diagnostics.includes('agent.provider_failed')) {
    const known = Object.entries(turnMessages).find(([code]) => diagnostics.includes(code));
    if (known) return known[1];
  }
  return userMessage(text, TURN_FALLBACK);
}
