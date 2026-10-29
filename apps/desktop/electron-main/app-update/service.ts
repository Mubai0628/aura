import { mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import type { AppUpdateCommand, AppUpdateState } from '../../shared/app-updates.js';
import { UNAVAILABLE_UPDATE_STATE } from '../../shared/app-updates.js';
import { downloadPackage, type FetchFunction } from './download.js';
import { newestRelease, type FeedRelease } from './feed.js';
import {
  copiedBefore,
  dismissNotice,
  readInstallState,
  readNotice,
  requestRestore,
  runSetup,
  SetupRefusal,
  withdrawRestore,
  type Installation,
  type RunProgram,
} from './installation.js';

const FIRST_CHECK_MS = 10_000;
const CHECK_EVERY_MS = 4 * 60 * 60_000;
const PROGRESS_EVERY_MS = 200;
// The launcher looks for this window's ready report every 500 ms; it may also never come (a
// version started without the launcher), so the record is read again for two minutes at most.
const TRIAL_REREAD_MS = 1_000;
const TRIAL_REREADS = 120;

export interface AppUpdateDependencies {
  installation: Installation | null;
  current: string;
  fetch: FetchFunction;
  feedUrl: string;
  /** `feedUrl` is a test release's own listing (AURA_UPDATE_FEED_URL). */
  testFeed?: boolean;
  /** Main's diagnostics log: the setup program's own words, which the page does not show. */
  log?: (event: string, fields: Record<string, unknown>) => void;
  userAgent: string;
  publish: (state: AppUpdateState) => void;
  requestRestart: () => void;
  run?: RunProgram;
  now?: () => Date;
}

interface Settings {
  auto_check: boolean;
  checked_at: string | null;
}

const reasonOf = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);
const HAS_CHINESE = /[㐀-鿿]/u;

/** Why a check failed, in words the page can show (an English network error is not shown). */
const checkFailure = (error: unknown): string =>
  HAS_CHINESE.test(reasonOf(error))
    ? reasonOf(error)
    : '检查更新失败，可能是网络或代理暂时不通。请稍后点“检查更新”重试。';

/** Why a download stopped part way and how it goes on: what arrived is kept for the next one. */
function downloadFailure(error: unknown): string {
  const code = typeof error === 'object' && error !== null ? Reflect.get(error, 'code') : undefined;
  if (typeof code === 'string' && /^E[A-Z]+$/u.test(code)) {
    return `更新包没能写入磁盘（${code}）。已下载的部分会保留，腾出磁盘空间后点“继续下载”接着下载。`;
  }
  const reason = reasonOf(error);
  return HAS_CHINESE.test(reason)
    ? `${reason}。已下载的部分会保留，点“继续下载”从中断处接着下载。`
    : '下载中断，可能是网络或代理暂时不通。已下载的部分会保留，网络恢复后点“继续下载”从中断处接着下载。';
}

/** Checking, downloading and applying Aura's own updates, and going back a version. */
export class AppUpdates {
  #state: AppUpdateState;
  #latest: FeedRelease | null = null;
  #download: AbortController | null = null;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #reread: ReturnType<typeof setTimeout> | null = null;
  #rereads = 0;
  #lastProgress = 0;

  constructor(private readonly dependencies: AppUpdateDependencies) {
    this.#state = {
      ...UNAVAILABLE_UPDATE_STATE,
      available: dependencies.installation !== null,
      current: dependencies.current,
    };
  }

  get state(): AppUpdateState {
    return this.#state;
  }

  #set(change: Partial<AppUpdateState>): AppUpdateState {
    this.#state = { ...this.#state, ...change };
    this.dependencies.publish(this.#state);
    return this.#state;
  }

  #now(): Date {
    return this.dependencies.now?.() ?? new Date();
  }

  #folder(): string {
    return join(this.dependencies.installation?.dataRoot ?? '', 'updates');
  }

  async #settings(): Promise<Settings> {
    try {
      const value = JSON.parse(
        await readFile(join(this.#folder(), 'settings.json'), 'utf8'),
      ) as Partial<Settings>;
      return {
        auto_check: value.auto_check !== false,
        checked_at: typeof value.checked_at === 'string' ? value.checked_at : null,
      };
    } catch {
      return { auto_check: true, checked_at: null };
    }
  }

  async #saveSettings(): Promise<void> {
    await mkdir(this.#folder(), { recursive: true });
    const settings: Settings = {
      auto_check: this.#state.auto_check,
      checked_at: this.#state.checked_at,
    };
    await writeFile(join(this.#folder(), 'settings.json'), `${JSON.stringify(settings)}\n`, 'utf8');
  }

  /** Reads what the installation and the launcher say now: previous and waiting versions, a notice. */
  async refresh(): Promise<AppUpdateState> {
    const installation = this.dependencies.installation;
    if (!installation) return this.#state;
    const install = await readInstallState(installation.root);
    const notice = await readNotice(installation.dataRoot, install.active);
    // Right after 重启并更新 this Aura is the waiting version on trial: the record names it the
    // active one, with the version before it, once the launcher sees this window ready.
    const onTrial =
      install.pending === installation.current && install.active !== installation.current;
    this.#rereadWhile(onTrial);
    const waiting =
      !onTrial && install.pending && install.pending !== install.active ? install.pending : null;
    return this.#set({
      previous: install.previous,
      waiting,
      notice,
      rollback_needs_data: copiedBefore(installation.dataRoot, installation.current),
      ...(waiting && this.#state.phase === 'idle' ? { phase: 'ready' as const } : {}),
    });
  }

  async start(): Promise<void> {
    if (!this.dependencies.installation) {
      this.dependencies.publish(this.#state);
      return;
    }
    const settings = await this.#settings();
    this.#set({ auto_check: settings.auto_check, checked_at: settings.checked_at });
    await this.refresh();
    this.#schedule(FIRST_CHECK_MS);
  }

  stop(): void {
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = null;
    this.#rereadWhile(false);
    this.#download?.abort();
  }

  #rereadWhile(onTrial: boolean): void {
    if (this.#reread) clearTimeout(this.#reread);
    this.#reread = null;
    if (!onTrial || this.#rereads >= TRIAL_REREADS) return;
    this.#rereads += 1;
    this.#reread = setTimeout(() => {
      this.#reread = null;
      void this.refresh().catch(() => this.#rereadWhile(true));
    }, TRIAL_REREAD_MS);
  }

  #schedule(delay: number): void {
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = null;
    if (!this.#state.auto_check || !this.#state.available) return;
    this.#timer = setTimeout(() => {
      void this.#check(true).finally(() => this.#schedule(CHECK_EVERY_MS));
    }, delay);
  }

  check(): Promise<AppUpdateState> {
    return this.#check(false);
  }

  /** An automatic check goes on to download and prepare what it found; only the restart is left. */
  async #check(automatic: boolean): Promise<AppUpdateState> {
    if (
      !this.#state.available ||
      ['checking', 'downloading', 'applying'].includes(this.#state.phase)
    ) {
      return this.#state;
    }
    this.#set({ phase: 'checking', error: null });
    try {
      const response = await this.dependencies.fetch(this.dependencies.feedUrl, {
        headers: {
          Accept: 'application/vnd.github+json',
          'User-Agent': this.dependencies.userAgent,
          'X-GitHub-Api-Version': '2022-11-28',
        },
      });
      if (!response.ok) throw new Error(`检查更新失败（HTTP ${response.status}）`);
      this.#latest = newestRelease(
        await response.json(),
        this.#state.current,
        this.dependencies.testFeed ? this.dependencies.feedUrl : undefined,
      );
      const checked = this.#now().toISOString();
      const latest = this.#latest
        ? {
            version: this.#latest.version,
            published_at: this.#latest.published_at,
            size: this.#latest.asset.size,
            notes: this.#latest.notes,
          }
        : null;
      const waitingIsLatest =
        this.#state.waiting !== null && this.#state.waiting === latest?.version;
      const fetching = automatic && latest !== null && !waitingIsLatest;
      this.#set({
        // Still checking until the download takes over: it starts from any phase but its own.
        phase: waitingIsLatest
          ? 'ready'
          : fetching
            ? 'checking'
            : latest
              ? 'available'
              : 'up_to_date',
        latest,
        checked_at: checked,
      });
      await this.#saveSettings();
      if (fetching) return await this.download();
    } catch (error) {
      this.#set({ phase: 'error', error: checkFailure(error) });
    }
    return this.#state;
  }

  async download(): Promise<AppUpdateState> {
    const installation = this.dependencies.installation;
    const latest = this.#latest;
    if (!installation || !latest || ['downloading', 'applying'].includes(this.#state.phase)) {
      return this.#state;
    }
    this.#download = new AbortController();
    // Published at most every PROGRESS_EVERY_MS; a download that stops reports all that arrived.
    let arrived = { done: 0, total: latest.asset.size };
    this.#set({ phase: 'downloading', error: null, progress: arrived });
    try {
      const file = await downloadPackage({
        fetch: this.dependencies.fetch,
        url: latest.asset.url,
        size: latest.asset.size,
        folder: this.#folder(),
        name: latest.asset.name,
        signal: this.#download.signal,
        onProgress: (done, total) => {
          arrived = { done, total };
          const now = Date.now();
          if (now - this.#lastProgress >= PROGRESS_EVERY_MS || done === total) {
            this.#lastProgress = now;
            this.#set({ progress: { done, total } });
          }
        },
      });
      this.#download = null;
      this.#set({ phase: 'applying' });
      const waiting = await runSetup(installation, ['--apply-update', file], this.dependencies.run);
      await this.#clearPackages();
      return this.#set({ phase: 'ready', waiting, progress: null });
    } catch (error) {
      const paused = this.#download?.signal.aborted === true;
      this.#download = null;
      if (paused) return this.#set({ phase: 'paused', progress: arrived });
      // A download that stopped on its own waits like a paused one.
      if (this.#state.phase === 'downloading') {
        return this.#set({ phase: 'paused', error: downloadFailure(error), progress: arrived });
      }
      this.dependencies.log?.('app_update.setup_refused', {
        action: 'apply_update',
        reason: reasonOf(error),
      });
      // Something on this computer held a folder the update replaces; the package itself is fine
      // and stays, so the next try installs it without downloading it again.
      if (error instanceof SetupRefusal && error.packageIntact) {
        return this.#set({
          phase: 'error',
          error: `${error.message}更新包已保留，再点“下载更新”会直接安装，不用重新下载。`,
        });
      }
      // Otherwise the package may be damaged: the next try downloads it again instead of
      // offering the same file.
      await this.#clearPackages();
      return this.#set({
        phase: 'error',
        error:
          '更新没能安装，已删除这次下载的更新包。点“下载更新”会重新下载；仍然失败时请重新运行安装程序修复。',
      });
    }
  }

  async #clearPackages(): Promise<void> {
    for (const name of await readdir(this.#folder()).catch(() => [] as string[])) {
      if (name.endsWith('.aura-update') || name.endsWith('.part')) {
        await rm(join(this.#folder(), name), { force: true });
      }
    }
  }

  pause(): AppUpdateState {
    this.#download?.abort();
    return this.#state;
  }

  restart(): AppUpdateState {
    if (this.#state.waiting) this.dependencies.requestRestart();
    return this.#state;
  }

  async rollBack(restoreData: boolean): Promise<AppUpdateState> {
    const installation = this.dependencies.installation;
    if (!installation || !this.#state.previous) return this.#state;
    if (!restoreData && copiedBefore(installation.dataRoot, installation.current)) {
      return this.#set({
        phase: 'error',
        error: `更新到 ${installation.current} 时升级了数据，${this.#state.previous} 读不了升级后的数据；回到上一版本要同时恢复更新前的数据。`,
        rollback_needs_data: true,
      });
    }
    let waiting: string;
    try {
      // The user's choice now decides; a request an earlier attempt left must not outlive it.
      if (restoreData) await requestRestore(installation.dataRoot, installation.current);
      else await withdrawRestore(installation.dataRoot);
      waiting = await runSetup(installation, ['--roll-back'], this.dependencies.run);
    } catch (error) {
      // The launcher reads a request at the next version switch; one left behind here would put
      // the old data back at a later rollback the user made without asking for it.
      const withdrawn = await withdrawRestore(installation.dataRoot).then(
        () => true,
        () => false,
      );
      const reason = reasonOf(error);
      this.dependencies.log?.('app_update.setup_refused', { action: 'roll_back', reason });
      return this.#set({
        phase: 'error',
        error: withdrawn
          ? reason
          : `${reason}；恢复数据的请求未能撤回，下次切换版本时可能恢复更新前的数据`,
      });
    }
    await dismissNotice(installation.dataRoot).catch(() => undefined);
    const state = this.#set({ phase: 'ready', waiting, error: null, progress: null });
    this.dependencies.requestRestart();
    return state;
  }

  async handle(command: AppUpdateCommand): Promise<AppUpdateState> {
    switch (command.command) {
      case 'state':
        return this.dependencies.installation ? this.refresh() : this.#state;
      case 'check':
        return this.check();
      case 'download':
        return this.download();
      case 'pause':
        return this.pause();
      case 'restart':
        return this.restart();
      case 'roll_back':
        return this.rollBack(command.restore_data);
      case 'dismiss_notice':
        if (this.dependencies.installation)
          await dismissNotice(this.dependencies.installation.dataRoot);
        return this.#set({ notice: null });
      case 'set_auto_check':
        this.#set({ auto_check: command.enabled });
        if (this.dependencies.installation) await this.#saveSettings();
        this.#schedule(FIRST_CHECK_MS);
        return this.#state;
    }
  }
}
