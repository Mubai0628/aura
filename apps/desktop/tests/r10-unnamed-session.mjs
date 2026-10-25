// B01/B03: real new-session input, automatic first-send naming and queue start.
import { runSessionAcceptance } from './r10-session-acceptance.mjs';
await runSessionAcceptance({ label: 'r10-unnamed-session', noTool: true, long: false });
