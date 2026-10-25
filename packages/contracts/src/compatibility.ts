import type { ContractVersion } from './generated/contracts.js';

export type ContractCompatibilityResult =
  | { compatible: true }
  | {
      compatible: false;
      reason: 'major_version_mismatch' | 'minor_version_too_new';
      supported: ContractVersion;
      incoming: ContractVersion;
    };

export const ensureContractCompatibility = (
  supported: ContractVersion,
  incoming: ContractVersion,
): ContractCompatibilityResult => {
  if (supported.major !== incoming.major) {
    return { compatible: false, reason: 'major_version_mismatch', supported, incoming };
  }
  if (incoming.minor > supported.minor) {
    return { compatible: false, reason: 'minor_version_too_new', supported, incoming };
  }
  return { compatible: true };
};
