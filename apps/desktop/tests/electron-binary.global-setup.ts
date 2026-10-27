import { createRequire } from 'node:module';

/**
 * Electron's package fetches its binary the first time it is loaded. It is loaded here once,
 * before the test workers start; test files that load it at the same moment in several workers
 * each started a fetch into the same folder, and one failed. Where it cannot be fetched, the tests
 * that need it still say so themselves.
 */
export default function setup(): void {
  try {
    createRequire(import.meta.url)('electron');
  } catch (error) {
    console.warn(`Electron binary not available before the tests: ${String(error)}`);
  }
}
