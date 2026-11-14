/**
 * What the window says about Core while Main supervises it: the cause, what it means and what to
 * do, in words. The technical reason stays in the Main log.
 */
import { errorEnvelope } from '../request-failure-envelopes.js';
import { isRecord } from './handshake.js';

export const RECONNECTING =
  '与 Aura Core 的连接断了，正在重新连接（Core 仍在运行）；这期间的操作会失败，连上后再试即可。';

/** A Core restarted while Aura stays open continues the turns it cut off by itself. */
export const RECONNECT_FAILED =
  '与 Aura Core 的连接没能恢复，正在重启 Core；正在运行的任务会暂停，重启后 Aura 会自动接着做，没有接上的在会话里发“继续”即可。';

export const INCOMPATIBLE_CORE =
  'Aura 界面与 Aura Core 不是同一个版本，Core 没有启动，所有功能暂不可用；请重新运行安装程序修复，或在启动器中回到上一版本。';

const MISSING =
  '找不到 aura-core.exe（可能被安全软件隔离或安装不完整），Aura 现在不能使用；请到安全软件的隔离区恢复它，或重新运行安装程序修复，然后重启 Aura。';
const BLOCKED =
  'Windows 或安全软件阻止了 aura-core.exe 启动，Aura 现在不能使用；请在安全软件中允许它后重启 Aura。';

/** Why aura-core.exe did not start, by the system's error code; no error yet reads as blocked. */
export const spawnFailureText = (error: unknown): string =>
  isRecord(error) && error.code === 'ENOENT' ? MISSING : BLOCKED;

const NOT_READY = 'Aura Core 正在启动或重启，这次操作没有发出；就绪后再试。';
const UNSUPPORTED = '当前 Aura Core 版本不支持这项功能，这次操作没有发出；请更新或修复安装。';

/**
 * A request for an interface the connected Core did not offer: while Core starts no interface is
 * offered yet. The envelope is what the window reads; the message names the interface for logs.
 */
export const unsupportedInterface = (name: string, starting: boolean): Error =>
  Object.assign(new Error(`Core does not support the ${name} interface`), {
    envelope: starting
      ? errorEnvelope('core_not_ready', NOT_READY, 'failed')
      : errorEnvelope('feature_unsupported', UNSUPPORTED, 'failed'),
  });
