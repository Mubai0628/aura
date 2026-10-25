import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ModelProfileDialog } from './ModelProfileDialog';
import { MODEL_ENDPOINT_ID, MODEL_PROFILE_ID, modelSettingsProjection } from './app-test-fixtures';

// The model's settings name its thinking level among the levels
// it offers, and saving any other setting keeps the level it has.

const update = vi.fn();
beforeEach(() => {
  update.mockReset();
  update.mockResolvedValue({
    ok: true,
    value: { revision: '9', value: { id: MODEL_PROFILE_ID, display_name: 'gpt-5.6-sol' } },
  });
  Object.assign(window, { aura: { v1: { updateModelProfile: update } } });
});
afterEach(cleanup);

const dialog = (level: 'high' | 'xhigh' | undefined, levels: string[]) => {
  const settings = structuredClone(modelSettingsProjection);
  const profile = settings.registry.models[MODEL_PROFILE_ID]!;
  if (level) profile.request_defaults.reasoning_effort = level;
  settings.reasoning_efforts = levels.length
    ? [{ model_profile_id: MODEL_PROFILE_ID, reasoning_efforts: levels as never }]
    : [];
  render(
    <ModelProfileDialog
      endpoint={settings.registry.endpoints[MODEL_ENDPOINT_ID]!}
      onClose={vi.fn()}
      onNotify={vi.fn()}
      onSaved={vi.fn(async () => {})}
      profile={profile}
      settings={settings}
    />,
  );
};

const sent = () => update.mock.calls[0]![0].request_defaults;

/**
 * The level choice: one of the model's levels, or its default. A segmented choice named
 * 会话默认思考程度, the level a conversation starts with; no level is 官方默认.
 */
const LEVEL_NAME = '会话默认思考程度';
const levelChoice = () => screen.getByRole('radiogroup', { name: LEVEL_NAME });
const chosenLevel = () =>
  within(levelChoice())
    .getAllByRole('radio')
    .find((radio) => (radio as HTMLInputElement).checked)
    ?.closest('label')?.textContent;

test('the levels the model offers, with the default first; a choice is saved', async () => {
  const user = userEvent.setup();
  dialog(undefined, ['low', 'medium', 'high']);
  expect(
    within(levelChoice())
      .getAllByRole('radio')
      .map((radio) => radio.closest('label')?.textContent),
  ).toEqual(['官方默认', '低', '中', '高']);
  expect(chosenLevel()).toBe('官方默认');
  await user.click(screen.getByRole('radio', { name: '中' }));
  expect(chosenLevel()).toBe('中');
  await user.click(screen.getByRole('button', { name: /保存/u }));
  expect(sent().reasoning_effort).toBe('medium');
});

// Older versions of Aura stored the top level of a model whose catalog names it max as xhigh;
// Core sends it as max, and the settings show what is sent.
test('a level an older Aura stored reads as the level the model is sent', async () => {
  const user = userEvent.setup();
  dialog('xhigh', ['low', 'high', 'max']);
  expect(chosenLevel()).toBe('最高');
  await user.click(screen.getByRole('button', { name: /保存/u }));
  expect(sent().reasoning_effort).toBe('max');
});

test('saving another setting keeps the level the model has', async () => {
  const user = userEvent.setup();
  dialog('high', ['low', 'medium', 'high']);
  await user.clear(screen.getByRole('spinbutton', { name: 'Top P（可选）' }));
  await user.type(screen.getByRole('spinbutton', { name: 'Top P（可选）' }), '0.9');
  await user.click(screen.getByRole('button', { name: /保存/u }));
  expect(sent().reasoning_effort).toBe('high');
  expect(sent().top_p).toBe(0.9);
});

test('a model offering no level has no choice and sends none', async () => {
  const user = userEvent.setup();
  dialog(undefined, []);
  expect(screen.queryByRole('radiogroup', { name: LEVEL_NAME })).toBeNull();
  await user.click(screen.getByRole('button', { name: /保存/u }));
  expect(sent().reasoning_effort).toBeNull();
});

// The levels a model offers can be
// listed in its settings, for a model the catalog does not know; the list is the profile's own
// once changed, and only then sent.
test('the levels a model offers can be listed; the list is sent once changed', async () => {
  const user = userEvent.setup();
  dialog(undefined, ['low', 'medium', 'high']);
  const levels = screen.getByRole('group', { name: '可选思考档位' });
  const checked = () =>
    within(levels)
      .getAllByRole('checkbox')
      .filter((box) => (box as HTMLInputElement).checked)
      .map((box) => box.closest('label')?.textContent);
  expect(checked()).toEqual(['低', '中', '高']);
  await user.click(screen.getByRole('checkbox', { name: '超高' }));
  await user.click(screen.getByRole('checkbox', { name: '最高' }));
  expect(checked()).toEqual(['低', '中', '高', '超高', '最高']);
  await user.click(screen.getByRole('radio', { name: '最高' }));
  // Unticking the default's level clears the default.
  await user.click(screen.getByRole('checkbox', { name: '最高' }));
  expect(chosenLevel()).toBe('官方默认');
  expect(screen.queryByRole('radio', { name: '最高' })).toBeNull();
  await user.click(screen.getByRole('checkbox', { name: '最高' }));
  await user.click(screen.getByRole('radio', { name: '最高' }));
  await user.click(screen.getByRole('button', { name: /保存/u }));
  expect(sent().reasoning_efforts).toEqual(['low', 'medium', 'high', 'xhigh', 'max']);
  expect(sent().reasoning_effort).toBe('max');
});

test('a list nobody changed is not sent: the catalog keeps deciding', async () => {
  const user = userEvent.setup();
  dialog(undefined, ['low', 'medium', 'high']);
  await user.click(screen.getByRole('button', { name: /保存/u }));
  expect(sent()).not.toHaveProperty('reasoning_efforts');
});

// Whether the model is sent images is the catalog's
// unless its user says otherwise; saving keeps what the profile has.
test('image input: automatic by default, a choice is saved, saving keeps it', async () => {
  const user = userEvent.setup();
  dialog(undefined, []);
  const images = screen.getByRole('radiogroup', { name: '图片输入' });
  const chosen = () =>
    within(images)
      .getAllByRole('radio')
      .find((radio) => (radio as HTMLInputElement).checked)
      ?.closest('label')?.textContent;
  expect(chosen()).toMatch(/^自动/u);
  await user.click(within(images).getByRole('radio', { name: '接收' }));
  await user.click(screen.getByRole('button', { name: /保存/u }));
  expect(sent().image_input).toBe(true);
  cleanup();
  update.mockClear();
  const settings = structuredClone(modelSettingsProjection);
  settings.registry.models[MODEL_PROFILE_ID]!.request_defaults.image_input = false;
  render(
    <ModelProfileDialog
      endpoint={settings.registry.endpoints[MODEL_ENDPOINT_ID]!}
      onClose={vi.fn()}
      onNotify={vi.fn()}
      onSaved={vi.fn(async () => {})}
      profile={settings.registry.models[MODEL_PROFILE_ID]!}
      settings={settings}
    />,
  );
  await user.click(screen.getByRole('button', { name: /保存/u }));
  expect(sent().image_input).toBe(false);
});

// Automatic sends images, unless the catalog
// knows the model takes none.
test('image input: automatic is 接收, except for a model the catalog knows takes no images', () => {
  for (const [catalog, label] of [
    ['unknown', '自动（接收）'],
    ['text only', '自动（不接收）'],
  ] as const) {
    const settings = structuredClone(modelSettingsProjection);
    const profile = settings.registry.models[MODEL_PROFILE_ID]!;
    if (catalog === 'unknown') profile.catalog = null;
    else profile.catalog!.snapshot.input_modalities = ['text'];
    render(
      <ModelProfileDialog
        endpoint={settings.registry.endpoints[MODEL_ENDPOINT_ID]!}
        onClose={vi.fn()}
        onNotify={vi.fn()}
        onSaved={vi.fn(async () => {})}
        profile={profile}
        settings={settings}
      />,
    );
    const images = screen.getByRole('radiogroup', { name: '图片输入' });
    const chosen = within(images)
      .getAllByRole('radio')
      .find((radio) => (radio as HTMLInputElement).checked)
      ?.closest('label')?.textContent;
    expect(chosen, catalog).toBe(label);
    cleanup();
  }
});
