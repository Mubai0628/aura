/**
 * The page code the built-in browser runs, shared rather than copied: the accessibility snapshot
 * and its refs, the elements refs name, input through DevTools and key names. Only the name of
 * the tool that reads a page again differs, and it is passed to the page's debugger.
 */
export {
  PageDebugger,
  type ConsoleEntry,
  type DevTools,
} from '../../../desktop/electron-main/browser-host/cdp.js';
export {
  boxesOf,
  elementBox,
  elementPoint,
  fieldAttributes,
  nodeOf,
  sensitiveField,
  type Point,
} from '../../../desktop/electron-main/browser-host/elements.js';
export {
  click,
  hover,
  insertText,
  mark,
  press,
  wheel,
} from '../../../desktop/electron-main/browser-host/input.js';
export { parseKey } from '../../../desktop/electron-main/browser-host/keys.js';
export {
  BOXED,
  renderSnapshot,
  type AxNode,
  type Box,
} from '../../../desktop/electron-main/browser-host/snapshot.js';
