import { mainRoute } from './generated/routes.js';
import { validatePackageRequest, validatePackageResponse } from './generated/package-validator.js';
import type { PackageMainCommand, PackageMainParams, PackageMainResult } from './package-api.js';
export const isPackageCommand = (command: string): command is PackageMainCommand =>
  mainRoute(command)?.owner === 'package';
export const isPackageParams = <C extends PackageMainCommand>(
  command: C,
  value: unknown,
): value is PackageMainParams[C] => validatePackageRequest({ command, value });
export function parsePackageResult<C extends PackageMainCommand>(
  command: C,
  value: unknown,
): PackageMainResult[C] {
  if (!validatePackageResponse({ command, value }))
    throw new Error('Invalid package result contract');
  return value as PackageMainResult[C];
}
