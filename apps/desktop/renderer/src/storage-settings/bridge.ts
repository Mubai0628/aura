import type { StorageApiV1 } from '../../../shared/storage-api';

/** The preload's storage bridge, when this window has one. */
export const storage = (): StorageApiV1 | undefined => window.aura?.v1.storage;
