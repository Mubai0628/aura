/** Technical diagnostics remain in the journal; product copy explains the next action. */
const messages: Record<string, string> = {
  'core.tool_policy.config_unavailable':
    '工具设置需要恢复，暂不能开始新的工具调用。请到设置中查看恢复选项。',
  'core.security_authority.config_unavailable':
    '权限设置或其依赖需要恢复，暂不能开始需要授权的新操作。请到设置中查看恢复选项。',
  'core.model_authority.config_unavailable':
    '模型配置或其依赖需要恢复，暂不能开始新的模型调用。请到设置中查看恢复选项。',
  'core.config_recovery.unresolved':
    '尚有待确认的恢复记录，相关新操作已暂停。请到设置中检查当前恢复状态。',
  // A model service that
  // could not be reached or did not answer, what that means for the task, and what to do next.
  model_transport_connect_timeout:
    '连接模型服务超时，已重试 2 次仍没有连上，可能是网络或代理暂时不通。任务已暂停，过程输出仍可阅读；网络恢复后在输入框发“继续”即可接着做。',
  model_transport_firstevent_timeout:
    '模型很久没有开始回答，已重试 2 次。可能是服务繁忙，或思考程度较高、准备时间较长。任务已暂停；稍后在输入框发“继续”即可接着做，也可以在端点设置里调长 First Event Timeout。',
  model_transport_streamidle_timeout:
    '模型的回答中途停顿太久，连接已断开。任务已暂停，过程输出仍可阅读；在输入框发“继续”即可接着做。',
  model_transport_total_timeout:
    '这一步超过了端点设置的总时长上限。任务已暂停；在输入框发“继续”即可接着做，也可以在端点设置里调长总时长。',
  model_transport_failed:
    '连接模型服务失败，已重试仍没有成功，可能是网络或代理暂时不通。任务已暂停，过程输出仍可阅读；网络恢复后在输入框发“继续”即可接着做。',
  // What the model service did, and what to do next. A provider's own codes are
  // matched in Core's failure text ("agent.provider_failed: <code> (…)"): a bare `request_timeout`
  // is also Main's own "no reply in time" for any command.
  provider_sse_invalid:
    '模型服务返回的数据无法解析，这一步没有完成。任务已暂停，过程输出仍可阅读；在输入框发“继续”即可接着做。',
  'agent.transport_interrupted':
    '与模型服务的连接中途断开，无法确认这一步是否完成，所以没有自动重发。任务已暂停；在输入框发“继续”即可接着做。',
  model_reasoning_loop:
    '模型在思考中反复回到同一段内容，没有开始回答；已自动重新问了 2 次，仍然如此，任务已暂停。过程输出仍可阅读；可以在输入框发“继续”再试，或换一个思考档位或模型。',
  'context.compaction_failed':
    '压缩较早的对话没有成功，这一步没有完成。任务已暂停，过程输出仍可阅读；在输入框发“继续”即可接着做。',
  'agent.execution_interrupted':
    '这一步执行时出了意外的错误，任务已暂停，错误代码记在 Core 日志里。过程输出仍可阅读；在输入框发“继续”即可接着做。',
  'provider_failed: context_length_exceeded':
    '这次请求超过了模型的上下文上限。任务已暂停；在输入框发“继续”，Aura 会先压缩较早的内容再接着做。',
  'provider_failed: rate_limit_exceeded':
    '模型服务限流（请求太频繁或额度不足），已重试仍未成功。任务已暂停；稍后在输入框发“继续”即可接着做。',
  // Such a step is asked again
  // automatically, twice at most in a row; what is shown is the third failure.
  'provider_failed: request_timeout':
    '模型服务中途停止了这一步的回答（服务端或其代理超时），已自动重试 2 次仍未成功。任务已暂停，过程输出仍可阅读；稍后在输入框发“继续”即可接着做。',
  'provider_failed: server_is_overloaded':
    '模型服务繁忙（服务端过载），已自动重试 2 次仍未成功。任务已暂停，过程输出仍可阅读；稍后在输入框发“继续”即可接着做。',
  'provider_failed: overloaded_error':
    '模型服务繁忙（服务端过载），已自动重试 2 次仍未成功。任务已暂停，过程输出仍可阅读；稍后在输入框发“继续”即可接着做。',
  'provider_failed: service_unavailable':
    '模型服务暂时不可用，已自动重试 2 次仍未成功。任务已暂停，过程输出仍可阅读；稍后在输入框发“继续”即可接着做。',
  'provider_failed: internal_server_error':
    '模型服务的服务端出错（可能是它的网关或代理暂时不通），已重试仍未成功。任务已暂停；稍后在输入框发“继续”即可接着做。',
  'tools.configuration_revision_conflict': '工具配置已在其他位置更新，请重新载入后再保存。',
  'tools.fixed_selection_invalid': '所选工具已发生变化，请重新载入工具列表。',
  'tools.dynamic_limit_must_be_positive': '动态工具上限必须为大于零的整数。',
  'tools.command_identity_conflict': '这次保存与原请求不一致，请重新打开设置核对。',
  'agent.model_binding_required': '请先选择模型，再点击“继续队列”。消息已保留。',
  'configuration.model_profile_not_selected': '请选择一个模型。',
  'model.profile_unavailable': '这个模型当前不可用，请检查模型设置或选择其他模型。',
  'agent.queue_paused': '队列已暂停，确认后可继续。',
  'agent.session_missing': '这个会话已不可用，请重新打开会话。',
  'session.title_conflict': '会话名称已发生变化，请关闭后重新命名。',
  'agent.core_restarted': '服务已重新启动，消息已保留，确认后可继续队列。',
  'agent.user_stopped': '任务已停止，消息已保留，确认后可继续队列。',
  'agent.stop_pending':
    '任务还没有完全停下：有一步还在收尾（例如等待命令退出）。停止请求已经记下，这一步结束后任务就会停下；也可以稍后再点一次“停止”。',
  'agent.protocol_not_completed': '本轮没有按协议提交完成。已停止自动继续，过程输出仍可阅读。',
  'agent.deterministic_correction_exhausted':
    '同一类参数错误在纠正后仍未修复，已停止本轮自动执行。',
  'agent.mode_switch_requires_idle':
    '切换 Plan 或 Goal 只能在空闲时开始。这条消息不会进入队列或追加。',
  'agent.queue_must_be_resolved_before_mode_start':
    '队列里还有消息。请先处理队列，再开启 Plan 或 Goal。',
  'agent.mode_switch_not_allowed': '当前模式仍在持续。请先在近期活动中终止，再开启另一种模式。',
  'task_lineage.revision_stale': '模式状态已变化，请重新载入后再操作。',
  'agent.mode_disposition_required': '请先决定是继续当前模式，还是跳过它。',
  'models.binding.credential_required':
    '这个模型所在的连接还没有可用凭据，请到模型设置里重新保存密钥后再选择。',
  'models.binding.output_limit_required': '这个模型需要先设置最大输出 Token 上限，才能被选中。',
  'models.binding.profile_disabled': '这个模型已被停用，请在模型设置中启用或选择其他模型。',
  'models.binding.profile_missing': '这个模型已不存在，请在模型设置中重新选择。',
  'models.binding.endpoint_missing': '这个模型所属的连接已不存在，请在模型设置中重新配置。',
  'agent.skills_need_new_turn':
    '选择的 Skill 只在开始新的一轮时使用：先移除 Skill 再追加，或等当前任务结束后发送。',
  'agent.skills_unavailable':
    '选择的 Skill 现在不能用（可能已停用、撤销信任或内容变了）。请移除它，或到 设置 → Skill 查看。',
  'agent.skills_too_many': '一次最多选择 20 个 Skill。',
  'hooks.turn_blocked':
    '一个 Hook 阻止了本轮开始，下面写明了是哪个 Hook 与原因。可在设置 → Hook 中查看或停用它。',
  'hooks.step_blocked':
    '一个 Hook 阻止了这一步调用模型，任务已中断。下面写明了是哪个 Hook 与原因。',
  'hooks.completion_blocked':
    'Hook 连续 3 次没有接受回答，已停止自动继续。过程输出仍可阅读，请在输入框说明下一步，或在设置 → Hook 中调整它。',
};
/** The newer Core areas refuse as `<code>: <sentence>`; the sentence is written for the user. */
const CODE_PREFIX = /^[a-z_]+(?:\.[a-z0-9_]+)+:\s*/u;
/** An internal code left inside a sentence (`agent.store_unknown`), unlike `fs.write` or `a.md`. */
const INTERNAL_CODE = /[a-z]+(?:\.[a-z0-9_]+)*\.[a-z0-9]+_[a-z0-9_]+/u;

/** A field of an error envelope, an Error or any other object that carries one. */
const field = (value: unknown, name: 'code' | 'message'): string | undefined => {
  if (typeof value !== 'object' || value === null) return undefined;
  const found = (value as Record<string, unknown>)[name];
  return typeof found === 'string' ? found : undefined;
};

/**
 * What the user reads for a failure from Core or Main: a known code gets its product sentence;
 * Core's own Chinese sentence is shown without its code; English exception text, transport details
 * and internal identifiers never reach the page — the caller's fallback says what to do instead.
 * Accepts an error envelope (`{ code, message }`), an Error or a string.
 */
export function userMessage(value: unknown, fallback = '操作未完成，请刷新状态后重试。'): string {
  const code = field(value, 'code');
  if (code !== undefined && messages[code] !== undefined) return messages[code]!;
  const text = typeof value === 'string' ? value : (field(value, 'message') ?? '');
  // Core quotes filenames with 「」. They are user content, even when a name resembles a code.
  const diagnostics = text.replace(/「[^」]*」/gu, '');
  if (
    diagnostics.includes('provider_tool_call_binding_invalid') ||
    diagnostics.includes('wire name ')
  ) {
    const matched = /wire name (\S+)/u.exec(diagnostics);
    const name = matched?.[1]?.slice(0, 64);
    return name
      ? `模型返回的工具名称「${name}」不在这次请求的工具清单里。任务已中断，不会自动重放已完成的写入。`
      : '模型返回的工具名称不在这次请求的工具清单里。任务已中断，不会自动重放已完成的写入。';
  }
  // The user's own Hook refused the message: its name and reason are the whole answer.
  const refused = /hooks\.message_blocked:\s*([\s\S]+)$/u.exec(text)?.[1]?.trim();
  if (refused) return refused.slice(0, 500);
  const known = Object.entries(messages).find(([code]) => diagnostics.includes(code));
  if (known) return known[1];
  // Never expose internal identifiers, transport details or English exception text.
  const sentence = text.replace(CODE_PREFIX, '').trim();
  return /[\u3400-\u9fff]/u.test(sentence) &&
    !INTERNAL_CODE.test(diagnostics.replace(CODE_PREFIX, ''))
    ? sentence.slice(0, 600)
    : fallback;
}

/** A restart settles an unfinished turn; nothing it had not finished is replayed. */
const RESTART_NEXT_STEP = '仍然不行时可以重启 Aura，启动时会收尾这一轮，不会重放没有完成的操作。';

/**
 * What a 停止 that did not finish says, instead of only "停止未确认": why,
 * what it means for the turn, and what to do next.
 */
export function stopFailureMessage(error: unknown): string {
  return userMessage(
    error,
    field(error, 'code') === 'request_timeout'
      ? `服务没有及时回复，这一轮可能仍在停止中。请稍后查看任务状态，仍在运行就再点一次“停止”；${RESTART_NEXT_STEP}`
      : `停止没有确认：服务没能确认这一轮已经停下。请稍后再点一次“停止”；${RESTART_NEXT_STEP}`,
  );
}

/** Whether a stop is still under way (its last step is ending) rather than refused. */
export const stopStillEnding = (error: unknown): boolean =>
  field(error, 'code') === 'agent.stop_pending' ||
  (field(error, 'message') ?? '').startsWith('agent.stop_pending');
