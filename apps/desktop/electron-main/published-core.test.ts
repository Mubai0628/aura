import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  resolvePublishedCoreExecutable,
  selectPackagedCoreExecutable,
  win32FilePath,
} from './published-core.js';

const hashOf = (bytes: string): string => createHash('sha256').update(bytes).digest('hex');
const ID_A = '4b486eb4-dd7f-4681-b8ac-7eece6ce8d25';
const ID_B = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee';

describe('published Core resolver', () => {
  const roots: string[] = [];
  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  const programDataRoot = async (): Promise<string> => {
    const programData = await mkdtemp(join(tmpdir(), 'aura-dev-binding-'));
    roots.push(programData);
    return programData;
  };

  const seed = async (opts: {
    programData?: string;
    id: string;
    bytes: string;
    path?: string;
    hash?: string;
    manifest?: Record<string, unknown>;
  }): Promise<{ programData: string; core: string }> => {
    const programData = opts.programData ?? (await programDataRoot());
    if (opts.programData === undefined) {
      // `programDataRoot` already pushed it; nothing else to do.
    }
    const root = join(programData, 'Aura', 'R06-Spike', opts.id);
    await mkdir(root, { recursive: true });
    const core = join(root, 'aura-core.exe');
    await writeFile(core, opts.bytes);
    const hex = opts.hash ?? hashOf(opts.bytes);
    await writeFile(
      join(root, 'execution.json'),
      JSON.stringify({
        installation_id: opts.id,
        revision: '1',
        core_image: {
          path: opts.path ?? `\\\\?\\${core}`,
          sha256: { algorithm: 'sha256', hex },
        },
        ...(opts.manifest ?? {}),
      }),
    );
    return { programData, core };
  };

  it('selects the unique ProgramData Core when path and hash match', async () => {
    const { programData, core } = await seed({ id: ID_A, bytes: 'published-core-bytes' });
    const resolved = await resolvePublishedCoreExecutable({
      programData,
      expectedSha256: hashOf('published-core-bytes'),
      installationId: ID_A,
    });
    expect(resolved.status).toBe('selected');
    if (resolved.status === 'selected') {
      expect(win32FilePath(resolved.path)).toBe(win32FilePath(core));
      expect(resolved.installationId).toBe(ID_A);
      expect(resolved.declaredSha256).toBe(hashOf('published-core-bytes'));
    }
  });

  it('refuses a Desktop-bound Core path even when a copy exists at the install root', async () => {
    const { programData } = await seed({
      id: ID_A,
      bytes: 'published-core-bytes',
      path: 'C:\\Users\\AuraAcceptance\\Desktop\\Aura-R09-Pro-135911\\aura-core.exe',
    });
    const resolved = await resolvePublishedCoreExecutable({
      programData,
      expectedSha256: hashOf('published-core-bytes'),
      installationId: ID_A,
    });
    expect(resolved.status).toBe('published_path_mismatch');
  });

  it('distinguishes a different published version from a missing installation', async () => {
    const { programData } = await seed({ id: ID_A, bytes: 'old-published-core' });
    await expect(
      resolvePublishedCoreExecutable({
        programData,
        expectedSha256: hashOf('this-build-core'),
      }),
    ).resolves.toMatchObject({ status: 'not_published_for_this_version' });
    const empty = await programDataRoot();
    await expect(
      resolvePublishedCoreExecutable({
        programData: empty,
        expectedSha256: hashOf('this-build-core'),
      }),
    ).resolves.toMatchObject({ status: 'installation_missing' });
  });

  it('refuses an installation id mismatch and an unknown expected version', async () => {
    const { programData } = await seed({ id: ID_A, bytes: 'published-core-bytes' });
    await expect(
      resolvePublishedCoreExecutable({
        programData,
        expectedSha256: hashOf('published-core-bytes'),
        installationId: ID_B,
      }),
    ).resolves.toMatchObject({ status: 'installation_id_mismatch' });
    await expect(
      resolvePublishedCoreExecutable({ programData, expectedSha256: 'not-a-hash' }),
    ).resolves.toMatchObject({ status: 'expected_version_unknown' });
  });

  it('returns ambiguous when two published Cores match the same version', async () => {
    const first = await seed({ id: ID_A, bytes: 'published-core-bytes' });
    await seed({ programData: first.programData, id: ID_B, bytes: 'published-core-bytes' });
    await expect(
      resolvePublishedCoreExecutable({
        programData: first.programData,
        expectedSha256: hashOf('published-core-bytes'),
      }),
    ).resolves.toMatchObject({ status: 'ambiguous' });
  });

  it('refuses a published file whose bytes are not the declared hash', async () => {
    const { programData, core } = await seed({
      id: ID_A,
      bytes: 'published-core-bytes',
      hash: hashOf('declared-but-not-present'),
    });
    await writeFile(core, 'published-core-bytes');
    await expect(
      resolvePublishedCoreExecutable({
        programData,
        expectedSha256: hashOf('published-core-bytes'),
      }),
    ).resolves.toMatchObject({ status: 'not_published_for_this_version' });
  });
});

describe('packaged Core selection', () => {
  const bundle = 'C:\\bundle\\aura-core.exe';
  const publishedPath = `C:\\ProgramData\\Aura\\R06-Spike\\${ID_A}\\aura-core.exe`;
  const selected = {
    status: 'selected' as const,
    path: publishedPath,
    installationId: ID_A,
    declaredSha256: 'a'.repeat(64),
    installations: [],
  };

  it('uses the selected published Core when present', () => {
    expect(selectPackagedCoreExecutable({ published: selected, bundleCore: bundle, env: {} })).toBe(
      publishedPath,
    );
  });

  it('ordinary packaged spawn keeps the bundle Core when none is published', () => {
    expect(
      selectPackagedCoreExecutable({
        published: { status: 'installation_missing', mismatchedComponents: [], installations: [] },
        bundleCore: bundle,
        env: {},
      }),
    ).toBe(bundle);
  });

  it('Directed spawn refuses a missing published Core before bundle fallback', () => {
    expect(() =>
      selectPackagedCoreExecutable({
        published: { status: 'installation_missing', mismatchedComponents: [], installations: [] },
        bundleCore: bundle,
        env: { AURA_ACCEPTANCE_DIRECTED: '1' },
      }),
    ).toThrow(
      'bound execution Core spawn refused: published Core installation_missing; bundle fallback is not allowed',
    );
  });

  it('matrix spawn refuses an ambiguous published Core before bundle fallback', () => {
    expect(() =>
      selectPackagedCoreExecutable({
        published: { status: 'ambiguous', mismatchedComponents: [], installations: [] },
        bundleCore: bundle,
        env: { AURA_ACCEPTANCE_MATRIX: 'C:\\payload\\acceptance\\shell-matrix.mjs' },
      }),
    ).toThrow(
      'bound execution Core spawn refused: published Core ambiguous; bundle fallback is not allowed',
    );
  });
});
