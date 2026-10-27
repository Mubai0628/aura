import js from '@eslint/js';
import { builtinModules } from 'node:module';
import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';
import globals from 'globals';
import tseslint from 'typescript-eslint';

const rendererFiles = ['apps/desktop/renderer/**/*.{ts,tsx}'];
const disabledNodeGlobals = Object.fromEntries(
  Object.keys(globals.node).map((name) => [name, 'off']),
);
const rendererForbiddenModules = [
  'electron',
  ...new Set(builtinModules.flatMap((name) => [name, `node:${name.replace(/^node:/, '')}`])),
];

// The Chrome extension runs in Chrome: no Node built-ins outside its build config and tests.
const extensionFiles = ['apps/chrome-extension/src/**/*.ts'];

export default tseslint.config(
  {
    ignores: [
      '.aura-dev/**',
      // Claude Code keeps its session worktrees here: other checkouts, not this tree.
      '.claude/**',
      '.tmp/**',
      '**/dist/**',
      '**/node_modules/**',
      '**/playwright-report/**',
      '**/test-results/**',
      'coverage/**',
      'packages/contracts/src/generated/**',
      'apps/desktop/shared/generated/**',
      'target/**',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['**/*.{js,mjs,cjs,ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2024,
      globals: globals.node,
      sourceType: 'module',
    },
  },
  {
    files: rendererFiles,
    languageOptions: {
      globals: { ...globals.browser, ...disabledNodeGlobals },
    },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      ...reactHooks.configs.flat.recommended.rules,
      'no-restricted-imports': [
        'error',
        {
          paths: rendererForbiddenModules.map((name) => ({
            name,
            message: 'Renderer code must use the versioned preload API.',
          })),
          patterns: [
            {
              group: ['node:*', 'child_process/*', 'fs/*', 'path/*'],
              message: 'Renderer code cannot import Node built-ins.',
            },
          ],
        },
      ],
      'no-restricted-syntax': [
        'error',
        ...rendererForbiddenModules.map((name) => ({
          selector: `ImportExpression[source.value='${name}']`,
          message: 'Renderer code cannot dynamically import Node or Electron.',
        })),
      ],
      'react-refresh/only-export-components': ['warn', { allowConstantExport: true }],
    },
  },
  {
    files: extensionFiles,
    ignores: ['**/*.test.ts'],
    languageOptions: {
      globals: { ...globals.browser, ...globals.serviceworker, ...disabledNodeGlobals },
    },
    rules: {
      'no-restricted-imports': [
        'error',
        {
          paths: rendererForbiddenModules.map((name) => ({
            name,
            message: 'The extension runs in Chrome.',
          })),
          patterns: [{ group: ['node:*'], message: 'The extension runs in Chrome.' }],
        },
      ],
    },
  },
);
