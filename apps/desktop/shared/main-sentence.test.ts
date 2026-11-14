import { expect, test } from 'vitest';

import { mainSentence } from './main-sentence.js';

// Main's own sentence, without the wrapping Electron adds on its way back to the window.

test("Main's sentence comes back without Electron's wrapping", () => {
  expect(
    mainSentence(
      new Error(
        "Error invoking remote method 'aura:v1:notifications': Error: 通知设置没能写入磁盘。",
      ),
    ),
  ).toBe('通知设置没能写入磁盘。');
  expect(mainSentence(new Error('plain'))).toBe('plain');
  expect(mainSentence('text')).toBe('text');
});
