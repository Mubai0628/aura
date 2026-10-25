import type { AuraWindowApi } from '../../shared/aura-api';

declare global {
  interface Window {
    readonly aura: AuraWindowApi;
  }
}

export {};
