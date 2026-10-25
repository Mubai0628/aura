import { requestGitCommand } from './git-requests.js';
import { requestPackageCommand } from './package-requests.js';
import { parseMemoryCoreResult } from '../shared/memory-validation.js';
import type {
  GitCoreCommand,
  GitCoreParams,
  PackageCoreCommand,
  PackageCoreParams,
  MemoryCoreCommand,
  MemoryCoreParams,
  MemoryCoreResult,
} from '../shared/generated/routes.js';
import type { ConfirmedRequest } from './secret-requests.js';
/** All domain requests retain the supervisor's single outcome-confirmation authority. */
export class CoreDomainRequests {
  constructor(private readonly confirm: ConfirmedRequest) {}
  git<C extends GitCoreCommand>(command: C, params: GitCoreParams[C]) {
    return requestGitCommand(command, params, this.confirm);
  }
  package<C extends PackageCoreCommand>(command: C, params: PackageCoreParams[C]) {
    return requestPackageCommand(command, params, this.confirm);
  }
  memory<C extends MemoryCoreCommand>(
    command: C,
    params: MemoryCoreParams[C],
  ): Promise<MemoryCoreResult[C]> {
    return this.confirm({
      command,
      accept: (value) => parseMemoryCoreResult(command, value),
      send: (transport) => transport.request(command, params, 1, 45_000),
    });
  }
}
