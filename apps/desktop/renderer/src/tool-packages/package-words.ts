import type { PackageMainCommand, PackageMainParams } from '../../../shared/package-api';
import { answerMissing, connectionCause, refusedAction } from '../agent-session/failure-text';

/**
 * What a tool-package action that did not happen says: which action, why, and what stays as it
 * was; one Core did not answer says it may have happened and to check the refreshed list.
 */

interface ActionWords {
  /** The action that did not happen, as a clause. */
  failed: string;
  /** What that means and what to do. */
  rest: string;
  /** The action as done, for one that changes something; reads have none. */
  done?: string;
}

const WORDS: Record<Exclude<PackageMainCommand, 'packages.set_enabled'>, ActionWords> = {
  'packages.list': { failed: '工具包列表没有读到', rest: '请重试。' },
  'packages.import_directory': {
    failed: '工具包没有导入',
    rest: '列表不变；请重试。',
    done: '导入',
  },
  'packages.import_archive': { failed: '工具包没有导入', rest: '列表不变；请重试。', done: '导入' },
  'packages.version.get': { failed: '这个版本的详情没有读到', rest: '请重试。' },
  'packages.conformance.prepare': { failed: '验证没有准备好', rest: '工具包保持原样；请重试。' },
  'packages.conformance.start': {
    failed: '验证没有开始',
    rest: '工具包保持原样；请重试。',
    done: '开始验证',
  },
  'packages.conformance.get': { failed: '验证结果没有读到', rest: '请重试。' },
  'packages.conformance.cancel': {
    failed: '验证没有取消',
    rest: '验证可能仍在进行；请重试。',
    done: '取消验证',
  },
  'packages.install': { failed: '工具包没有安装', rest: '列表不变；请重试。', done: '安装' },
  'packages.revoke_trust': {
    failed: '工具包的信任没有撤销',
    rest: '它仍可运行；请重试。',
    done: '撤销信任',
  },
  'packages.uninstall': { failed: '工具包没有卸载', rest: '它仍已安装；请重试。', done: '卸载' },
  'packages.credential.bind': {
    failed: '凭据没有绑定',
    rest: '绑定保持原样；请重试。',
    done: '绑定',
  },
  'packages.credential.clear': {
    failed: '凭据绑定没有解除',
    rest: '工具仍使用这个凭据；请重试。',
    done: '解除绑定',
  },
};

function wordsFor<C extends PackageMainCommand>(
  command: C,
  params: PackageMainParams[C],
): ActionWords {
  if (command !== 'packages.set_enabled') return WORDS[command as keyof typeof WORDS];
  return (params as PackageMainParams['packages.set_enabled']).enabled
    ? { failed: '工具包没有启用', rest: '它仍是停用的；请重试。', done: '启用' }
    : { failed: '工具包没有停用', rest: '它仍在启用中；请重试。', done: '停用' };
}

/** A package action Core refused or did not answer. */
export function packageFailure<C extends PackageMainCommand>(
  command: C,
  params: PackageMainParams[C],
  error: unknown,
): string {
  const words = wordsFor(command, params);
  if (words.done !== undefined && answerMissing(error))
    return `没有等到 Aura Core 的答复（${connectionCause(error)}），可能已经${words.done}；看刷新后的列表核对，再决定是否重做。`;
  return refusedAction(error, words.failed, words.rest);
}

/** A package action that never left Aura. */
export function packageNotSent<C extends PackageMainCommand>(
  command: C,
  params: PackageMainParams[C],
): string {
  const words = wordsFor(command, params);
  return `${words.failed}（Aura 内部通信失败），${words.rest}`;
}
