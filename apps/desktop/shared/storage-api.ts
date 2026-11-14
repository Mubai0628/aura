import type { AgentMainParams, AgentMainResult } from './generated/routes.js';
import type { ResultEnvelope } from './aura-api.js';

type Request<M extends keyof AgentMainParams> = (
  request: AgentMainParams[M],
) => Promise<ResultEnvelope<AgentMainResult[M]>>;
type Read<M extends keyof AgentMainParams> = () => Promise<ResultEnvelope<AgentMainResult[M]>>;

/** 设置 → 系统 → 存储: where the data and the installation are, moving the data, and old data. */
export interface StorageApiV1 {
  overview: Read<'storage.overview'>;
  /** Whether the data can go to a folder, or back to the default places. */
  check: Request<'storage.check'>;
  /** Asks for the move; it happens at the next start. */
  requestMove: Request<'storage.move.request'>;
  withdrawMove: Read<'storage.move.withdraw'>;
  dismissFailure: Read<'storage.failure.dismiss'>;
  /** What older versions and other installations left, which nothing uses. */
  oldData: Read<'storage.old_data.list'>;
  /** Removes the picked items; one that is in use is kept whole. */
  removeOldData: Request<'storage.old_data.remove'>;
  /** Main's own folder dialog. */
  selectFolder: Read<'storage.select_folder'>;
  openFolder: Request<'storage.open_folder'>;
  /** Closes Aura so the launcher starts it again and moves the data first. */
  restart: Read<'storage.restart'>;
  /** Opens the installed setup program to move the installation, and closes Aura. */
  moveInstall: Read<'storage.move_install'>;
}
