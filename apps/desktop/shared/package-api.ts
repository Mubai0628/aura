import type {
  PackageMainCommand,
  PackageMainParams,
  PackageMainResult,
} from './generated/routes.js';
import type { ResultEnvelope } from './aura-api.js';
/** The tool-package page's bridge. Imports name no path: Main shows its own dialog. */
export interface PackageApiV1 {
  invoke<C extends PackageMainCommand>(
    command: C,
    params: PackageMainParams[C],
  ): Promise<ResultEnvelope<PackageMainResult[C]>>;
}
export type { PackageMainCommand, PackageMainParams, PackageMainResult };
