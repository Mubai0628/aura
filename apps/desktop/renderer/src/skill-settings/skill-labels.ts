import type { SkillSource, SkillState, SkillView } from '@aura/contracts/generated';

export const skillSourceLabel = (source: SkillSource): string => {
  switch (source) {
    case 'aura_store':
      return '已导入 Aura';
    case 'agent_global':
      return '我的 .agents';
    case 'workspace':
      return '当前工作区';
    case 'plugin':
      return '插件';
  }
};

export const skillStateLabel = (skill: SkillView): string => {
  const state: SkillState = skill.state;
  switch (state) {
    case 'ready':
      return skill.missing_references.length ? '可用 · 部分引用的文件缺失' : '可用';
    case 'disabled':
      return skill.trusted_fingerprint ? '已停用' : '未启用，未信任';
    case 'not_trusted':
      return '未信任，模型看不到';
    case 'content_changed':
      return '内容已变化，需要重新信任';
    case 'invalid':
      return '无法使用';
    case 'unavailable':
      return '无法读取';
  }
};

/** The colour a Skill's state is shown in. */
export const skillStateTone = (skill: SkillView): 'success' | 'warning' | 'danger' | 'muted' =>
  skill.state === 'ready'
    ? skill.missing_references.length
      ? 'warning'
      : 'success'
    : skill.state === 'disabled'
      ? 'muted'
      : skill.state === 'invalid' || skill.state === 'unavailable'
        ? 'danger'
        : 'warning';

/** Core refuses Skill changes as `skills.<reason>: <sentence>`; the sentence is the user's. */
export function skillError(value: unknown, fallback = '操作未完成，请刷新后重试。'): string {
  const text = value instanceof Error ? value.message : typeof value === 'string' ? value : '';
  const sentence = text.replace(/^[a-z_]+(?:\.[a-z_]+)+:\s*/u, '').trim();
  return sentence && sentence !== text.trim() && /[㐀-鿿]/u.test(sentence)
    ? sentence.slice(0, 600)
    : fallback;
}

export const bytesLabel = (bytes: number): string =>
  bytes < 1024
    ? `${bytes} 字节`
    : bytes < 1024 * 1024
      ? `${(bytes / 1024).toFixed(1)} KB`
      : `${(bytes / 1024 / 1024).toFixed(1)} MB`;
