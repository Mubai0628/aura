import { registerMediaCommands } from './core-channel.js';
import { exact, mediaIdentity, readMediaCommand, record } from './protocol.js';
import { checkedMediaResult } from './results.js';
import { MediaSource } from './source.js';
import {
  MediaFailure,
  MediaRefusal,
  type MediaCommand,
  type MediaCore,
  type MediaDecoded,
  type MediaIdentity,
  type MediaOutcome,
} from './types.js';

export type DecodeMedia = (
  command: MediaCommand,
  source: MediaSource,
  signal: AbortSignal,
) => Promise<MediaDecoded>;
interface Job {
  command: MediaCommand;
  source: MediaSource;
  stop: AbortController;
  timer: ReturnType<typeof setTimeout>;
}
const key = (identity: MediaIdentity) =>
  `${identity.runtime_instance_id}\0${identity.session_id}\0${identity.command_id}`;
export class MediaHost {
  readonly #jobs = new Map<string, Job>();
  readonly #seen = new Set<string>();
  #queue: Promise<void> = Promise.resolve();
  #owner: MediaCore | undefined;
  #unsubscribe: (() => void) | undefined;
  #runtime: string | undefined;
  #accepting = true;
  constructor(
    private readonly options: {
      decode?: DecodeMedia;
      log?: (event: string, fields: Record<string, unknown>) => void;
    } = {},
  ) {}

  async #answer(core: MediaCore, identity: MediaIdentity, outcome: MediaOutcome): Promise<void> {
    try {
      await core.requestWithBudget('media.host.result', { ...identity, outcome }, 15_000);
    } catch {
      this.options.log?.('media.result_not_delivered', { command_id: identity.command_id });
    }
  }

  #run(core: MediaCore, value: unknown, runtime: string): void {
    if (!this.#accepting || (this.#runtime !== undefined && this.#runtime !== runtime)) return;
    const identity = mediaIdentity(value, runtime);
    if (!identity) return;
    const command = readMediaCommand(value, runtime);
    if (!command) {
      void this.#answer(core, identity, {
        outcome: 'refused',
        message: '拒绝媒体请求：身份、字段或取帧范围无效，不接受文件路径。',
      });
      return;
    }
    const id = key(identity);
    // A duplicate must not complete the original pending command with a second answer.
    if (this.#seen.has(id)) return;
    this.#seen.add(id);
    if (this.#seen.size > 512) this.#seen.delete(this.#seen.values().next().value!);
    if (this.#jobs.size >= 4) {
      void this.#answer(core, identity, {
        outcome: 'refused',
        message: '媒体解码正在忙，请稍后重试。',
      });
      return;
    }
    const source = new MediaSource(command, core, Date.now() + command.timeout_ms);
    const stop = new AbortController();
    const timer = setTimeout(
      () => stop.abort(new MediaFailure('媒体解码超时，已停止读取。')),
      command.timeout_ms,
    );
    const job = { command, source, stop, timer };
    this.#jobs.set(id, job);
    const next = this.#queue
      .then(async () => {
        try {
          if (stop.signal.aborted) throw stop.signal.reason;
          const decode = this.options.decode ?? (await import('./decoder.js')).decodeMedia;
          const decoded = await decode(command, source, stop.signal);
          if (stop.signal.aborted) throw stop.signal.reason;
          const result = checkedMediaResult(decoded, command);
          source.close();
          await this.#answer(core, identity, { outcome: 'completed', result });
        } catch (error) {
          const message =
            error instanceof MediaRefusal || error instanceof MediaFailure
              ? error.message
              : '媒体解码失败，Chromium无法读取这个视频。';
          source.close();
          if (this.#jobs.get(id) === job)
            await this.#answer(core, identity, {
              outcome: error instanceof MediaRefusal ? 'refused' : 'failed',
              message,
            });
        } finally {
          clearTimeout(timer);
          source.close();
          stop.abort();
          if (this.#jobs.get(id) === job) this.#jobs.delete(id);
        }
      })
      .catch(() => undefined);
    this.#queue = next;
  }

  bindCore(core: MediaCore): void {
    this.dispose();
    this.#owner = core;
    this.#accepting = core.subscribe === undefined;
    this.#unsubscribe = core.subscribe?.((change) => {
      if (this.#owner !== core) return;
      const runtime = change.runtime_instance_id;
      if (
        change.payload.state !== 'ready' ||
        (this.#runtime !== undefined && runtime !== this.#runtime)
      )
        this.#stopJobs();
      this.#accepting = change.payload.state === 'ready';
      this.#runtime = this.#accepting ? runtime : undefined;
    });
    registerMediaCommands(core, {
      command: (value, runtime) => {
        if (this.#owner === core) this.#run(core, value, runtime);
      },
      cancel: (value, runtime) => {
        if (this.#owner !== core) return;
        const identity = mediaIdentity(value, runtime);
        if (
          !identity ||
          !record(value) ||
          !exact(value, 'command_id,runtime_instance_id,session_id')
        )
          return;
        const job = this.#jobs.get(key(identity));
        if (job) {
          this.#jobs.delete(key(identity));
          clearTimeout(job.timer);
          job.source.close();
          job.stop.abort(new MediaFailure('媒体请求已停止。'));
        }
      },
    });
  }
  dispose(): void {
    this.#owner = undefined;
    this.#unsubscribe?.();
    this.#unsubscribe = undefined;
    this.#runtime = undefined;
    this.#accepting = false;
    this.#stopJobs();
  }
  #stopJobs(): void {
    for (const job of this.#jobs.values()) {
      clearTimeout(job.timer);
      job.source.close();
      job.stop.abort(new MediaFailure('媒体连接已结束。'));
    }
    this.#jobs.clear();
    this.#seen.clear();
  }
}
