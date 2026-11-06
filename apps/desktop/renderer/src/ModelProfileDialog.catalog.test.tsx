import type {
  ModelProfileCatalogEntriesView,
  ProviderCatalogMatchCandidate,
} from '@aura/contracts/generated';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { isP05MainParams } from '../../shared/p05-validation';
import { ModelProfileDialog } from './ModelProfileDialog';
import { MODEL_ENDPOINT_ID, MODEL_PROFILE_ID, modelSettingsProjection } from './app-test-fixtures';

// A model that several catalog Providers list under its name shows them to choose from. The
// choice is written at once, with the entry as Core listed it, and the settings are read again.

const hash = (digit: string) => ({ algorithm: 'blake3' as const, hex: digit.repeat(64) });
const listed = (
  chosen: ProviderCatalogMatchCandidate | null,
  count = 4,
): ModelProfileCatalogEntriesView => ({
  catalog_revision: '7',
  chosen,
  entries: [
    ['openai', 'gpt-5.6-sol', 'OpenAI', 'a'],
    ['azure', 'gpt-5.6-sol', 'Azure', 'b'],
    ['azure-cognitive-services', 'gpt-5.6-sol', 'Azure', 'c'],
    ['relay', 'openai/gpt-5.6-sol', 'Relay', 'd'],
  ]
    .slice(0, count)
    .map(([provider, model, name, digit]) => ({
      catalog_model_id: model!,
      catalog_object_hash: hash(digit!),
      catalog_provider_id: provider!,
      provider_display_name: name!,
    })),
  model_profile_id: MODEL_PROFILE_ID,
});

const read = vi.fn();
const apply = vi.fn();
const notify = vi.fn();
const saved = vi.fn(async () => {});
beforeEach(() => {
  for (const mock of [read, apply, notify, saved]) mock.mockReset();
  saved.mockResolvedValue(undefined);
  Object.assign(window, {
    aura: {
      v1: {
        applyModelProfileCatalogUpdate: apply,
        getModelProfileCatalogEntries: read,
        updateModelProfile: vi.fn(),
      },
    },
  });
});
afterEach(cleanup);

const dialog = () => {
  const settings = structuredClone(modelSettingsProjection);
  render(
    <ModelProfileDialog
      endpoint={settings.registry.endpoints[MODEL_ENDPOINT_ID]!}
      onClose={vi.fn()}
      onNotify={notify}
      onSaved={saved}
      profile={settings.registry.models[MODEL_PROFILE_ID]!}
      settings={settings}
    />,
  );
};
const field = () => screen.findByRole('combobox', { name: '目录条目' });
const options = (select: HTMLElement) =>
  within(select)
    .getAllByRole('option')
    .map((option) => option.textContent);

test('each Provider that lists the name can be chosen; the entry is written as listed', async () => {
  const user = userEvent.setup();
  read.mockResolvedValue({ ok: true, value: listed(null) });
  apply.mockResolvedValue({ ok: true, value: { changed: true, revision: '2', value: {} } });
  dialog();
  const select = await field();
  expect(read).toHaveBeenCalledWith({ model_profile_id: MODEL_PROFILE_ID });
  expect(options(select)).toEqual([
    '未选择',
    'OpenAI',
    'Azure（azure）',
    'Azure（azure-cognitive-services）',
    'Relay · openai/gpt-5.6-sol',
  ]);
  await user.selectOptions(select, 'openai/gpt-5.6-sol');
  await waitFor(() => expect(saved).toHaveBeenCalledOnce());
  const request = apply.mock.calls[0]![0];
  expect(request).toEqual({
    catalog_entry: { catalog_model_id: 'gpt-5.6-sol', catalog_provider_id: 'openai' },
    expected_catalog_object_hash: hash('a'),
    expected_catalog_revision: '7',
    expected_model_profile_revision: '1',
    expected_models_revision: '1',
    model_profile_id: MODEL_PROFILE_ID,
  });
  expect(isP05MainParams('models.profile.apply_catalog_update', request)).toBe(true);
  expect(notify).not.toHaveBeenCalled();
});

test('the chosen entry shows as chosen, with nothing to unchoose', async () => {
  read.mockResolvedValue({
    ok: true,
    value: listed({ catalog_model_id: 'gpt-5.6-sol', catalog_provider_id: 'azure' }),
  });
  dialog();
  const select = (await field()) as HTMLSelectElement;
  expect(select.value).toBe('azure/gpt-5.6-sol');
  expect(options(select)).not.toContain('未选择');
});

test('a name only one entry matches, or a list that cannot be read, shows no choice', async () => {
  for (const answer of [
    { ok: true, value: listed(null, 1) },
    { ok: false, error: { code: 'models.catalog.unavailable', message: 'no catalog' } },
  ]) {
    read.mockReset();
    read.mockResolvedValue(answer);
    dialog();
    await waitFor(() => expect(read).toHaveBeenCalled());
    await Promise.resolve();
    expect(screen.queryByRole('combobox', { name: '目录条目' })).toBeNull();
    expect(screen.getByRole('button', { name: /保存/u })).toBeTruthy();
    cleanup();
  }
});

test('a refused choice says why and reads nothing again', async () => {
  const user = userEvent.setup();
  read.mockResolvedValue({ ok: true, value: listed(null) });
  apply.mockResolvedValue({
    ok: false,
    error: {
      code: 'models.catalog.entry_changed',
      message: '模型目录刚更新过，这一条已经变了，这次没有保存；请重新打开设置再选。',
    },
  });
  dialog();
  await user.selectOptions(await field(), 'relay/openai/gpt-5.6-sol');
  await waitFor(() => expect(notify).toHaveBeenCalledOnce());
  expect(notify.mock.calls[0]![0]).toMatchObject({
    message: '模型目录刚更新过，这一条已经变了，这次没有保存；请重新打开设置再选。',
    title: '目录条目未保存',
    tone: 'danger',
  });
  expect(saved).not.toHaveBeenCalled();
  expect(((await field()) as HTMLSelectElement).disabled).toBe(false);
});
