// The one declaration of which build outputs are bound execution images.
//
// Two entries have to agree about this set, and they must agree without importing each
// other's side effects: the execution image build publishes seven images into a
// candidate and records their digests, and the ordinary `corepack pnpm dev` entry binds
// the same digests as "the versions this run expects". Both write the same paths under
// `target/debug`, so cargo decides identity by fingerprint — a different `-p` set or a
// different `RUSTFLAGS` is a different unit of work, and the relink replaces the
// published file object with a new one.
import { resolve } from 'node:path';

export const REPOSITORY_ROOT = resolve(import.meta.dirname, '..');

/** Where a debug build writes the images the publication and the development entry bind. */
export const DEBUG_DIRECTORY = resolve(REPOSITORY_ROOT, 'target/debug');

/**
 * The Release Bundle components a debug build produces.
 *
 * `main_pipe_bridge` is a Node-API addon rather than a Rust binary, and its output is
 * `apps/desktop/dist/native/aura-main-pipe.node` from `build-main-pipe.mjs`. It was
 * previously declared as `target/debug/aura-main-pipe.node`, which no build step ever
 * creates, so the component could never bind. Its path is absolute because the other
 * entries are relative to `target/debug` and this one is not.
 */
export const DEVELOPMENT_OUTPUTS = {
  core: 'aura-core.exe',
  sandbox_host: 'aura-sandbox-host.exe',
  egress_proxy: 'aura-egress-proxy.exe',
  elevated_helper: 'aura-elevated-helper.exe',
  main_pipe_bridge: resolve(REPOSITORY_ROOT, 'apps/desktop/dist/native/aura-main-pipe.node'),
};

/**
 * The execution objects the protected publication places in the installation.
 *
 * These are recorded separately from the Release Bundle components because "the Runner
 * changed but Core did not" has to be a visible version mismatch: the installation's
 * runner is a separate file object that can be replaced without touching Core.
 */
export const EXECUTION_OUTPUTS = {
  core: 'aura-core.exe',
  runner: 'aura-command-runner.exe',
  helper: 'aura-execution-helper.exe',
  maintainer: 'aura-execution-maintainer.exe',
};
