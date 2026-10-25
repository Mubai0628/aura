import type { ContextSectionKind, PromptInspectorProjection } from '@aura/contracts/generated';

/** A count as the inspector shows it: grouped digits. */
export const amount = (value: number) => new Intl.NumberFormat('zh-CN').format(value);

/** Every share with two decimals. */
export const share = (part: number, whole: number) =>
  `${((part / Math.max(1, whole)) * 100).toFixed(2)}%`;

/** Each part of a request by a short name. */
export const SECTION_LABELS: Record<ContextSectionKind, string> = {
  core_system_prompt: '系统提示',
  permission_and_security: '权限与安全',
  loaded_tool_schemas: '工具定义',
  workspace_agents_instructions: '工作区说明',
  historical_snapshot_summary: '压缩摘要',
  agent_mode_prompt: '模式',
  hook_injections: 'Hook',
  current_task_state: '任务状态',
  recent_history: '近期对话',
  file_references_and_pending_attachments: '文件与附件',
  current_turn_working_set: '本轮工作集',
  steered_user_additions: '追加消息',
};

export const COMPACTION_LABELS: Record<PromptInspectorProjection['compaction']['state'], string> = {
  cancelled: '已取消',
  completed: '已压缩',
  failed: '压缩失败',
  not_required: '无需压缩',
  required: '需要压缩',
  running: '压缩中',
};

/** The provider protocol a step was sent with, by its short name. */
export const FORMAT_LABELS: Record<string, string> = {
  open_ai_responses: 'Responses',
  open_ai_chat_completions: 'Chat',
  anthropic_messages: 'Messages',
};
