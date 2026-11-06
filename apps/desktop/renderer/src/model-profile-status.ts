import type { ModelProfileConfig } from '@aura/contracts/generated';

/** What a model card says about whether its model can be used as it is. */
export const profileStatusLabel = (profile: ModelProfileConfig): string => {
  if (!profile.enabled) return '已停用';
  if (
    profile.request_defaults.max_output_tokens === null ||
    profile.request_defaults.max_output_tokens === undefined
  ) {
    if (
      profile.catalog?.snapshot.output_limit_tokens === null ||
      profile.catalog?.snapshot.output_limit_tokens === undefined
    ) {
      return '需配置输出上限';
    }
  }
  return '已配置';
};

/** A model ready to use: enabled, with an output limit of its own or from the catalog. */
export const isProfileReady = (profile: ModelProfileConfig): boolean =>
  profileStatusLabel(profile) === '已配置';

/** Whether `query`, lower-cased and trimmed, names the model or its provider model id. */
export const profileMatches = (profile: ModelProfileConfig, query: string): boolean =>
  query === '' ||
  profile.display_name.toLowerCase().includes(query) ||
  profile.provider_model_name.toLowerCase().includes(query);
