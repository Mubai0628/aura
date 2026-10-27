/** The side panel's entry: the service worker's state in, the user's requests out. */
import { chromeApi } from '../chrome-api.js';
import { PANEL_PORT, type PanelState } from '../panel-messages.js';
import { render, type CurrentTab, type Form } from './view.js';

const chrome = chromeApi();
const root = document.getElementById('app') as HTMLElement;
const port = chrome.runtime.connect({ name: PANEL_PORT });
let state: PanelState | null = null;
let tab: CurrentTab | null = null;
const form: Form = { code: '', session: '', scope: 'session' };

function draw(): void {
  if (state === null) return;
  render(
    root,
    { state, tab, form, now: Date.now() },
    {
      send: (request) => port.postMessage(request),
      edit: (change) => Object.assign(form, change),
    },
  );
}

async function refreshTab(): Promise<void> {
  const [active] = await chrome.tabs.query({ active: true, currentWindow: true });
  tab =
    active?.id === undefined
      ? null
      : {
          tab_id: active.id,
          window_id: active.windowId,
          url: active.url ?? '',
          title: active.title ?? '',
        };
  draw();
}

port.onMessage.addListener((message) => {
  state = message as PanelState;
  draw();
});
chrome.tabs.onActivated.addListener(() => void refreshTab());
chrome.tabs.onUpdated.addListener(() => void refreshTab());
// The time left for a tab handed to the user.
setInterval(() => {
  if (state?.assist) draw();
}, 1_000);
void refreshTab();
