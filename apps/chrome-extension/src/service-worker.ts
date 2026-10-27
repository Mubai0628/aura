/** The extension's service worker: everything runs in `Background`; this wires Chrome to it. */
import { Background } from './background.js';
import { chromeApi } from './chrome-api.js';

const chrome = chromeApi();
const background = new Background({
  chrome,
  hostName: __AURA_HOST_NAME__,
  timers: {
    set: (work, ms) => setTimeout(work, ms),
    clear: (timer) => clearTimeout(timer as ReturnType<typeof setTimeout>),
  },
  now: () => Date.now(),
  sleep: (ms) => new Promise((done) => setTimeout(done, ms)),
});
void chrome.sidePanel.setPanelBehavior({ openPanelOnActionClick: true });
background.start();
