/** The model profile fields whose value can keep 保存 from being pressed. */
export type ProfileField =
  | 'provider_model_name'
  | 'display_name'
  | 'context_window'
  | 'max_output_tokens'
  | 'historical_tokens';

export interface ProfileFieldValues {
  providerModelName: string;
  displayName: string;
  contextWindow: number | '';
  maxOutputTokens: number | '';
  historicalTokens: number;
}

/** Each field that cannot be saved as it is, with what to fix, in the order the form shows them. */
export function profileFieldIssues(
  values: ProfileFieldValues,
): Partial<Record<ProfileField, string>> {
  const issues: Partial<Record<ProfileField, string>> = {};
  if (values.providerModelName.length === 0) issues.provider_model_name = '请填写模型标识';
  if (values.displayName.trim().length === 0) issues.display_name = '请填写显示名称';
  if (values.contextWindow !== '' && !(values.contextWindow > 0))
    issues.context_window = '上下文窗口要大于 0';
  if (values.maxOutputTokens !== '' && !(values.maxOutputTokens > 0))
    issues.max_output_tokens = '最大输出要大于 0';
  if (!(values.historicalTokens >= 0)) issues.historical_tokens = '历史快照不能小于 0';
  return issues;
}
