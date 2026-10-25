import type { Buffer } from 'node:buffer';
import type { Page } from '@playwright/test';
import type { FixtureProvider } from './r10-session-provider.mjs';

export function captureSecretProducts(input: {
  page: Page;
  root: string;
  endpointId: string;
  output: string;
  provider: FixtureProvider;
  modelRequestBodies: unknown[];
  responses: unknown[];
}): Promise<Record<'ipc' | 'logs' | 'events' | 'projections' | 'context' | 'artifacts', Buffer>>;
