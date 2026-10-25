// Real production window; only the remote provider is replaced by SSE fixtures.
import { runSessionAcceptance } from './r10-session-acceptance.mjs';
const noTool = process.env.R10_U01_NO_TOOL === '1';
await runSessionAcceptance({
  label: noTool ? 'r10-u01-session-ui-no-tool' : 'r10-u01-session-ui',
  noTool,
});
