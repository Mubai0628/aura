import { beforeEach, expect, test } from 'vitest';
import { sessionChoices } from './session-choices';

// A conversation's composer choices are kept for the 300 conversations that chose most recently;
// past that, the one that chose longest ago is forgotten, also after the window reloads.

const MODEL = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAV';
beforeEach(() => localStorage.clear());

test('past 300 conversations the one that chose longest ago is forgotten', () => {
  const choices = sessionChoices<string>('aura.session-choices-test.v1');
  for (let n = 0; n < 300; n += 1) choices.choose(`ses_${n}`, MODEL, 'fast');
  // Choosing again makes a conversation the newest.
  choices.choose('ses_0', MODEL, 'ultrafast');
  choices.choose('ses_300', MODEL, 'fast');
  choices.forget();
  expect(choices.get('ses_0', MODEL)).toBe('ultrafast');
  expect(choices.get('ses_1', MODEL)).toBeUndefined();
  expect(choices.get('ses_2', MODEL)).toBe('fast');
  expect(choices.get('ses_300', MODEL)).toBe('fast');
});
