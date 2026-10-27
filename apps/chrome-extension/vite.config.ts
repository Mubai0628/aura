import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { defineConfig, type Plugin } from 'vite';

const root = import.meta.dirname;
const target = process.env.AURA_CHROME_TARGET === 'test' ? 'test' : 'production';
const PRODUCTION_HOST = 'com.aura.chrome_control';

/**
 * The native-messaging host this build reaches, as `aura-chrome-domain::protocol::native_host_name`
 * names it: the released host, or the one of the test namespace the build is for.
 */
function hostName(): string {
  if (target === 'production') return PRODUCTION_HOST;
  const namespace = process.env.AURA_CHROME_NAMESPACE;
  if (!namespace) throw new Error('a test build names its Aura namespace in AURA_CHROME_NAMESPACE');
  const digest = createHash('sha256').update(namespace).digest('hex');
  return `${PRODUCTION_HOST}.t${digest.slice(0, 12)}`;
}

/** The manifest with this build's public key, so the extension id is fixed, and the icon. */
function manifest(): Plugin {
  return {
    name: 'aura-chrome-manifest',
    generateBundle() {
      const { version } = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8')) as {
        version: string;
      };
      const key = readFileSync(resolve(root, 'keys', `${target}.pub`), 'utf8').trim();
      const document = {
        manifest_version: 3,
        name: target === 'production' ? 'Aura' : 'Aura (test)',
        version,
        key,
        minimum_chrome_version: '116',
        permissions: ['nativeMessaging', 'debugger', 'tabs', 'tabGroups', 'sidePanel', 'storage'],
        background: { service_worker: 'service-worker.js', type: 'module' },
        side_panel: { default_path: 'side-panel/index.html' },
        action: { default_title: 'Aura' },
        icons: { '128': 'icon.png' },
      };
      this.emitFile({
        type: 'asset',
        fileName: 'manifest.json',
        source: `${JSON.stringify(document, null, 2)}\n`,
      });
      this.emitFile({
        type: 'asset',
        fileName: 'icon.png',
        source: readFileSync(resolve(root, '../../resources/app-icon/aura-256.png')),
      });
    },
  };
}

export default defineConfig({
  root: resolve(root, 'src'),
  base: './',
  define: { __AURA_HOST_NAME__: JSON.stringify(hostName()) },
  plugins: [manifest()],
  build: {
    emptyOutDir: true,
    // A test run builds its own copy for its own namespace.
    outDir: process.env.AURA_CHROME_OUT_DIR ?? resolve(root, 'dist', target),
    target: 'chrome116',
    // Extension pages allow no inline script.
    modulePreload: false,
    rolldownOptions: {
      input: {
        'service-worker': resolve(root, 'src/service-worker.ts'),
        'side-panel': resolve(root, 'src/side-panel/index.html'),
      },
      output: {
        entryFileNames: '[name].js',
        chunkFileNames: 'chunks/[name]-[hash].js',
        assetFileNames: 'assets/[name]-[hash][extname]',
      },
    },
  },
});
