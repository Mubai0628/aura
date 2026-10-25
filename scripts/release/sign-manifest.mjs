// Signs a release's component-manifest.json with the release signing key: a detached DER CMS with
// one ECDSA P-256 / SHA-256 signer, written next to it as component-manifest.p7s.
//
//   node scripts/release/sign-manifest.mjs <version folder> [--signing-dir <dir>] [--test-dir <dir>]
//
// --test-dir reads the signing files from <dir>/signing and the passphrase from
// AURA_RELEASE_SIGNING_PASSPHRASE.
import { existsSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { findOpenssl, openssl } from './openssl.mjs';
import { askHidden } from './prompt.mjs';
import { ROOT_NAME, SIGNING_NAME } from './release-key.mjs';

function parse(argv) {
  const [folder, ...rest] = argv;
  if (!folder || folder.startsWith('--'))
    throw new Error(
      'usage: sign-manifest.mjs <version folder> [--signing-dir <dir>] [--test-dir <dir>]',
    );
  const options = { folder: resolve(folder) };
  for (let index = 0; index < rest.length; index += 2) {
    const [flag, value] = [rest[index], rest[index + 1]];
    if (!['--signing-dir', '--test-dir'].includes(flag) || !value)
      throw new Error(`unknown or incomplete option: ${flag}`);
    options[flag === '--test-dir' ? 'testDir' : 'signingDir'] = resolve(value);
  }
  return options;
}

export async function signManifest(options) {
  const signing = options.testDir
    ? join(options.testDir, 'signing')
    : (options.signingDir ?? join(process.env.APPDATA ?? '', 'Aura Release', 'signing'));
  const key = join(signing, `${SIGNING_NAME}.key.pem`);
  const certificate = join(signing, `${SIGNING_NAME}.pem`);
  const root = join(signing, `${ROOT_NAME}.pem`);
  for (const path of [key, certificate, root]) {
    if (!existsSync(path)) throw new Error(`release signing file missing: ${path}`);
  }
  const manifest = join(options.folder, 'component-manifest.json');
  const signature = join(options.folder, 'component-manifest.p7s');
  if (!existsSync(manifest)) throw new Error(`no component-manifest.json in ${options.folder}`);
  const secret = options.testDir
    ? process.env.AURA_RELEASE_SIGNING_PASSPHRASE
    : await askHidden('签名密钥口令：');
  if (!secret) throw new Error('AURA_RELEASE_SIGNING_PASSPHRASE is required with --test-dir');
  const program = findOpenssl();
  rmSync(signature, { force: true });
  openssl(
    program,
    [
      'cms',
      '-sign',
      '-binary',
      '-md',
      'sha256',
      '-nosmimecap',
      '-outform',
      'DER',
      '-signer',
      certificate,
      '-inkey',
      key,
      '-passin',
      'env:AURA_SIGNING_PASSPHRASE',
      '-in',
      manifest,
      '-out',
      signature,
    ],
    { secrets: { AURA_SIGNING_PASSPHRASE: secret } },
  );
  // The signer's certificate chains to the root and its purpose is code signing, not S/MIME.
  openssl(program, [
    'cms',
    '-verify',
    '-binary',
    '-inform',
    'DER',
    '-in',
    signature,
    '-content',
    manifest,
    '-CAfile',
    root,
    '-purpose',
    'any',
    '-out',
    process.platform === 'win32' ? 'NUL' : '/dev/null',
  ]);
  return signature;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  signManifest(parse(process.argv.slice(2)))
    .then((signature) => console.log(`签名：${signature}`))
    .catch((error) => {
      console.error(error.message);
      process.exit(1);
    });
}
