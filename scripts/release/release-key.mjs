// The Aura release key: a root certificate whose public part is compiled into Aura, and a signing
// certificate issued by it that signs each release's component manifest.
//
//   node scripts/release/release-key.mjs init --backup <folder on removable media>
//   node scripts/release/release-key.mjs renew-signing --backup <the same folder>
//   node scripts/release/release-key.mjs show
//
// The root key exists only in the backup folder, encrypted with its passphrase; it is needed again
// only to issue a new signing certificate. The signing key is encrypted with its own passphrase and
// kept in the signing folder (default %APPDATA%\Aura Release\signing). The certificate profiles are
// the ones crates/aura-windows-platform checks: build.rs for the root, release_trust.rs for the
// signer.
//
// --test-dir <folder> writes everything into that folder, takes the passphrases from
// AURA_RELEASE_ROOT_PASSPHRASE and AURA_RELEASE_SIGNING_PASSPHRASE, and leaves the repository's
// root certificate untouched.
import { randomBytes } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { findOpenssl, openssl } from './openssl.mjs';
import { askHidden, askNewPassphrase } from './prompt.mjs';

const REPOSITORY = resolve(import.meta.dirname, '../..');
export const REPOSITORY_ROOT_DER = join(
  REPOSITORY,
  'resources/release-trust/production/aura-production-release-root-v1.der',
);
export const ROOT_NAME = 'aura-production-release-root-v1';
export const SIGNING_NAME = 'aura-production-release-signing';
const ROOT_DAYS = 7305;
const SIGNING_DAYS = 1826;

const ROOT_CONFIG = `[ req ]
distinguished_name = subject
prompt = no
x509_extensions = root
[ subject ]
O = Aura Project
CN = Aura Production Release Root v1
[ root ]
basicConstraints = critical, CA:TRUE, pathlen:1
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always
`;

const SIGNING_CONFIG = `[ req ]
distinguished_name = subject
prompt = no
[ subject ]
O = Aura Project
CN = Aura Production Release Signing
[ signing ]
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature
extendedKeyUsage = codeSigning
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always
`;

const BACKUP_README = `Aura 发布根密钥备份

这个文件夹里的 ${ROOT_NAME}.key.pem 是 Aura 发布根证书的私钥，已用你设的根密钥口令加密。
它只在签发新的发布签名证书时用到（node scripts/release/release-key.mjs renew-signing）。

- 请把这个文件夹放在电脑以外的地方保管（如 U 盘），并另外记住口令。
- 丢了它：已安装的 Aura 将不能再接受更新，只能让用户手动重装新版本。
- 泄露了它和口令：别人可以发布让已安装的 Aura 接受的更新。
`;

function parse(argv) {
  const [command, ...rest] = argv;
  const options = { command };
  for (let index = 0; index < rest.length; index += 1) {
    const flag = rest[index];
    const value = rest[index + 1];
    if (!['--backup', '--signing-dir', '--test-dir'].includes(flag) || !value) {
      throw new Error(`unknown or incomplete option: ${flag}`);
    }
    options[flag.slice(2).replace('-dir', 'Dir')] = resolve(value);
    index += 1;
  }
  return options;
}

function locations(options) {
  if (options.testDir) {
    return {
      backup: join(options.testDir, 'backup'),
      signing: join(options.testDir, 'signing'),
      rootDer: join(options.testDir, `${ROOT_NAME}.der`),
      test: true,
    };
  }
  const appData = process.env.APPDATA;
  if (!options.signingDir && !appData) throw new Error('APPDATA is not set; pass --signing-dir');
  return {
    backup: options.backup,
    signing: options.signingDir ?? join(appData, 'Aura Release', 'signing'),
    rootDer: REPOSITORY_ROOT_DER,
    test: false,
  };
}

async function passphrase(kind, test, isNew) {
  if (test) {
    const name =
      kind === 'root' ? 'AURA_RELEASE_ROOT_PASSPHRASE' : 'AURA_RELEASE_SIGNING_PASSPHRASE';
    const value = process.env[name];
    if (!value) throw new Error(`${name} is required with --test-dir`);
    return value;
  }
  const label = kind === 'root' ? '根密钥口令' : '签名密钥口令';
  return isNew ? askNewPassphrase(label) : askHidden(`${label}：`);
}

/** A positive serial of 128 random bits, as OpenSSL hex. */
function serial() {
  const bytes = randomBytes(16);
  bytes[0] = (bytes[0] & 0x7f) | 0x40;
  return `0x${bytes.toString('hex')}`;
}

function newKey(program, path, secret) {
  openssl(
    program,
    [
      'genpkey',
      '-algorithm',
      'EC',
      '-pkeyopt',
      'ec_paramgen_curve:P-256',
      '-pkeyopt',
      'ec_param_enc:named_curve',
      '-aes-256-cbc',
      '-pass',
      'env:AURA_KEY_PASSPHRASE',
      '-out',
      path,
    ],
    { secrets: { AURA_KEY_PASSPHRASE: secret } },
  );
}

/** Issues the signing certificate from the root; the signing key is created first when absent. */
function issueSigning(program, where, rootKey, rootCert, secrets) {
  mkdirSync(where.signing, { recursive: true });
  const signingKey = join(where.signing, `${SIGNING_NAME}.key.pem`);
  const signingCert = join(where.signing, `${SIGNING_NAME}.pem`);
  const config = join(where.signing, 'signing.cnf');
  const request = join(where.signing, 'signing.csr');
  writeFileSync(config, SIGNING_CONFIG);
  try {
    if (!existsSync(signingKey)) newKey(program, signingKey, secrets.signing);
    openssl(
      program,
      [
        'req',
        '-new',
        '-key',
        signingKey,
        '-passin',
        'env:AURA_SIGNING_PASSPHRASE',
        '-config',
        config,
        '-out',
        request,
      ],
      { secrets: { AURA_SIGNING_PASSPHRASE: secrets.signing } },
    );
    openssl(
      program,
      [
        'x509',
        '-req',
        '-in',
        request,
        '-CA',
        rootCert,
        '-CAkey',
        rootKey,
        '-passin',
        'env:AURA_ROOT_PASSPHRASE',
        '-set_serial',
        serial(),
        '-sha256',
        '-days',
        String(SIGNING_DAYS),
        '-extfile',
        config,
        '-extensions',
        'signing',
        '-out',
        signingCert,
      ],
      { secrets: { AURA_ROOT_PASSPHRASE: secrets.root } },
    );
  } finally {
    rmSync(config, { force: true });
    rmSync(request, { force: true });
  }
  writeFileSync(join(where.signing, `${ROOT_NAME}.pem`), readFileSync(rootCert));
  openssl(program, ['verify', '-CAfile', rootCert, '-purpose', 'any', signingCert]);
  return signingCert;
}

function fingerprint(program, certificate) {
  return openssl(program, ['x509', '-in', certificate, '-noout', '-fingerprint', '-sha256'])
    .trim()
    .replace(/^.*=/u, '');
}

async function init(options) {
  const where = locations(options);
  if (!where.backup) throw new Error('init needs --backup <folder>, ideally on removable media');
  const rootKey = join(where.backup, `${ROOT_NAME}.key.pem`);
  for (const existing of [rootKey, where.rootDer, join(where.signing, `${SIGNING_NAME}.key.pem`)]) {
    if (existsSync(existing))
      throw new Error(`refusing to replace an existing release key: ${existing}`);
  }
  const program = findOpenssl();
  const secrets = {
    root: await passphrase('root', where.test, true),
    signing: await passphrase('signing', where.test, true),
  };
  mkdirSync(where.backup, { recursive: true });
  const rootCert = join(where.backup, `${ROOT_NAME}.pem`);
  const config = join(where.backup, 'root.cnf');
  writeFileSync(config, ROOT_CONFIG);
  try {
    newKey(program, rootKey, secrets.root);
    openssl(
      program,
      [
        'req',
        '-new',
        '-x509',
        '-key',
        rootKey,
        '-passin',
        'env:AURA_ROOT_PASSPHRASE',
        '-set_serial',
        serial(),
        '-sha256',
        '-days',
        String(ROOT_DAYS),
        '-config',
        config,
        '-out',
        rootCert,
      ],
      { secrets: { AURA_ROOT_PASSPHRASE: secrets.root } },
    );
  } finally {
    rmSync(config, { force: true });
  }
  mkdirSync(dirname(where.rootDer), { recursive: true });
  openssl(program, ['x509', '-in', rootCert, '-outform', 'DER', '-out', where.rootDer]);
  writeFileSync(join(where.backup, 'README.txt'), BACKUP_README);
  const signingCert = issueSigning(program, where, rootKey, rootCert, secrets);
  console.log(`根证书（编进 Aura）：${where.rootDer}`);
  console.log(`根证书 SHA-256 指纹：${fingerprint(program, rootCert)}`);
  console.log(`根密钥备份：${where.backup}`);
  console.log(`签名证书：${signingCert}`);
}

async function renewSigning(options) {
  const where = locations(options);
  if (!where.backup) throw new Error('renew-signing needs --backup <the root key backup folder>');
  const rootKey = join(where.backup, `${ROOT_NAME}.key.pem`);
  const rootCert = join(where.backup, `${ROOT_NAME}.pem`);
  if (!existsSync(rootKey) || !existsSync(rootCert))
    throw new Error(`no root key backup in ${where.backup}`);
  const program = findOpenssl();
  const secrets = {
    root: await passphrase('root', where.test, false),
    signing: await passphrase(
      'signing',
      where.test,
      !existsSync(join(where.signing, `${SIGNING_NAME}.key.pem`)),
    ),
  };
  console.log(`新的签名证书：${issueSigning(program, where, rootKey, rootCert, secrets)}`);
}

function show(options) {
  const where = locations(options);
  const program = findOpenssl();
  for (const certificate of [
    join(where.signing, `${ROOT_NAME}.pem`),
    join(where.signing, `${SIGNING_NAME}.pem`),
  ]) {
    if (!existsSync(certificate)) {
      console.log(`没有：${certificate}`);
      continue;
    }
    console.log(
      openssl(program, [
        'x509',
        '-in',
        certificate,
        '-noout',
        '-subject',
        '-enddate',
        '-fingerprint',
        '-sha256',
      ]).trim(),
    );
  }
}

export async function main(argv) {
  const options = parse(argv);
  if (options.command === 'init') return init(options);
  if (options.command === 'renew-signing') return renewSigning(options);
  if (options.command === 'show') return show(options);
  throw new Error(
    'usage: release-key.mjs init|renew-signing|show [--backup <dir>] [--signing-dir <dir>] [--test-dir <dir>]',
  );
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
