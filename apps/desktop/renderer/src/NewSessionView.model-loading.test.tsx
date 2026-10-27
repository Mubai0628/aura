import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { NewSessionView } from './NewSessionView';
import {
  createLocalConversation,
  resetComposerDraftsForTests,
  useComposerDrafts,
} from './agent-session/composer-drafts';
import { resetSendingForTests } from './agent-session/sending';

// A new conversation reads its model list when it opens. A message sent before the list has
// arrived waits for it and goes with the default model, instead of being refused for having none.

type Catalog = {
  choices: { id: string; label: string; provider: string }[];
  defaultId?: string;
  loading: boolean;
};
const catalog = vi.hoisted(() => {
  let value: Catalog = { choices: [], loading: true };
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set(next: Catalog) {
      value = next;
      for (const listener of listeners) listener();
    },
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
});
vi.mock('./agent-session/useDraftModelChoices', async () => {
  const { useSyncExternalStore } = await import('react');
  return { useDraftModelChoices: () => useSyncExternalStore(catalog.subscribe, catalog.get) };
});
vi.mock('./model-projection-store', () => ({
  readSessionModelSelector: async () => ({
    payload: { binding_revision: '0', selected_model_profile_id: null },
  }),
}));

const create = vi.fn(),
  bind = vi.fn(),
  submit = vi.fn();
const loaded: Catalog = {
  choices: [{ id: 'model', label: 'Flash', provider: 'Provider' }],
  defaultId: 'model',
  loading: false,
};

beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  resetSendingForTests();
  catalog.set({ choices: [], loading: true });
  create.mockReset().mockResolvedValue({ ok: true, value: { session_id: 'server-session' } });
  bind.mockReset().mockResolvedValue({ ok: true, value: { revision: '1' } });
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        createSession: create,
        setSessionModelBinding: bind,
        submitAgentMessage: submit,
        startAgent: vi.fn(),
        renameSession: vi.fn().mockResolvedValue({ ok: true, value: {} }),
        getPermissionDefaults: async () => ({ ok: true, value: { mode: 'read_only' } }),
        listCommands: async () => ({ ok: true, value: { commands: [] } }),
      },
    },
  });
});
afterEach(cleanup);

function Fixture({ id }: { id: string }) {
  const draft = useComposerDrafts().find((item) => item.id === id)!;
  return (
    <NewSessionView
      draft={draft}
      onAccepted={vi.fn()}
      onInspect={vi.fn()}
      onOpenSettings={vi.fn()}
    />
  );
}

async function sendBeforeTheModelsArrive() {
  const user = userEvent.setup();
  render(<Fixture id={createLocalConversation('workspace')} />);
  await user.type(screen.getByRole('textbox', { name: '输入消息' }), '你好');
  await user.click(screen.getByRole('button', { name: '发送消息' }));
}

test('a message sent while the model list is still being read goes with the default model once it arrives', async () => {
  await sendBeforeTheModelsArrive();
  expect(screen.queryByText('请先选择已配置的模型。')).toBeNull();
  expect(create).not.toHaveBeenCalled();
  act(() => catalog.set(loaded));
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith({
      session_id: 'server-session',
      text: '你好',
      destination: { lane: 'next_turn' },
    }),
  );
  expect(bind).toHaveBeenCalledWith(expect.objectContaining({ model_profile_id: 'model' }));
  expect(screen.queryByText('请先选择已配置的模型。')).toBeNull();
});

test('a list that arrives with no model to use still says so, and nothing is sent', async () => {
  await sendBeforeTheModelsArrive();
  act(() => catalog.set({ choices: [], loading: false }));
  expect(await screen.findByText('请先选择已配置的模型。')).toBeTruthy();
  expect(create).not.toHaveBeenCalled();
  expect(submit).not.toHaveBeenCalled();
  expect((screen.getByRole('textbox', { name: '输入消息' }) as HTMLTextAreaElement).value).toBe(
    '你好',
  );
});
