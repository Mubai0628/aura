// The E2E build starts the production Main unchanged; the harness launches it through
// this entry.
await import(new URL('../../electron-main/main.js', import.meta.url).href);
