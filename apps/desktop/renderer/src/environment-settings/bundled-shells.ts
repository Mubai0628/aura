/**
 * The PowerShell Aura ships with. Core verifies the installed runtime against
 * `runtimes/powershell.json` before it runs a shell, so this is the version that runs; a test keeps
 * the two the same.
 */
export const BUNDLED_POWERSHELL = { version: '7.6.5', architecture: 'x64' } as const;
