import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { AgentProfileEditor } from './AgentProfileEditor';

const read = vi.fn();
const save = vi.fn();
const preview = vi.fn();
const profile = (more: Record<string, unknown> = {}) => ({
  session_id: 'ses_main',
  workspace_id: 'wsp_main',
  name: '测试会话',
  description: '负责回归测试',
  description_source: 'model',
  revision: 1,
  updated_at: null,
  input_tokens: 50,
  output_tokens: 10,
  usage_complete: true,
  generation: null,
  ...more,
});
beforeEach(() => {
  read.mockReset().mockResolvedValue({ ok: true, value: profile() });
  preview.mockReset();
  save.mockReset().mockResolvedValue({
    ok: true,
    value: profile({ description_source: 'user', revision: 2, description: '人工编辑的职责' }),
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentProfile: read,
        setAgentProfile: save,
        previewAgentProfile: preview,
      },
    },
  });
});
afterEach(cleanup);

test('existing conversation can edit its description and see auxiliary model usage', async () => {
  render(<AgentProfileEditor sessionId="ses_main" visible />);
  const field = await screen.findByRole('textbox', { name: '会话职责简介' });
  expect((field as HTMLTextAreaElement).value).toBe('负责回归测试');
  expect(screen.getByText(/简介生成用量：输入 50，输出 10/u)).toBeTruthy();
  fireEvent.change(field, { target: { value: '人工编辑的职责' } });
  fireEvent.click(screen.getByRole('button', { name: '保存简介' }));
  await waitFor(() =>
    expect(save).toHaveBeenCalledWith({
      session_id: 'ses_main',
      expected_revision: 1,
      description: '人工编辑的职责',
    }),
  );
  expect(screen.queryByText(/自动更新不会覆盖/u)).toBeNull();
});

test('a failed generation keeps the previous description and discloses incomplete usage', async () => {
  read.mockResolvedValue({
    ok: true,
    value: profile({
      usage_complete: false,
      generation: {
        source_run_id: 'agt_source',
        source_turn_id: 'trn_source',
        model_profile_id: null,
        state: 'failed',
        input_tokens: 50,
        output_tokens: 10,
        usage_complete: false,
        message: '这次简介未能生成，保留旧简介，不影响聊天。',
      },
    }),
  });
  render(<AgentProfileEditor sessionId="ses_main" visible />);
  const field = await screen.findByRole('textbox', { name: '会话职责简介' });
  expect((field as HTMLTextAreaElement).value).toBe('负责回归测试');
  expect(screen.getByText(/简介生成用量.*待核对/u)).toBeTruthy();
  expect(screen.getByText(/这次简介未能生成/u)).toBeTruthy();
  expect(save).not.toHaveBeenCalled();
});

test('refresh while editing keeps the original expected revision instead of silently overwriting newer text', async () => {
  const { rerender } = render(<AgentProfileEditor sessionId="ses_main" visible />);
  const field = await screen.findByRole('textbox', { name: '会话职责简介' });
  fireEvent.change(field, { target: { value: '从第一版开始的编辑' } });
  read.mockResolvedValue({
    ok: true,
    value: profile({ revision: 2, description: '另一处刚保存的简介' }),
  });
  rerender(<AgentProfileEditor sessionId="ses_main" visible={false} />);
  rerender(<AgentProfileEditor sessionId="ses_main" visible />);
  await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
  fireEvent.click(screen.getByRole('button', { name: '保存简介' }));
  await waitFor(() =>
    expect(save).toHaveBeenCalledWith({
      session_id: 'ses_main',
      expected_revision: 1,
      description: '从第一版开始的编辑',
    }),
  );
});

test('switching conversations never carries the previous draft or edit revision into a different agent', async () => {
  const { rerender } = render(<AgentProfileEditor key="ses_main" sessionId="ses_main" visible />);
  const field = await screen.findByRole('textbox', { name: '会话职责简介' });
  fireEvent.change(field, { target: { value: '旧会话未保存的草稿' } });
  read.mockResolvedValue({
    ok: true,
    value: profile({ session_id: 'ses_other', revision: 0, description: '另一会话的职责' }),
  });
  rerender(<AgentProfileEditor key="ses_other" sessionId="ses_other" visible />);
  await waitFor(() =>
    expect(
      (screen.getByRole('textbox', { name: '会话职责简介' }) as HTMLTextAreaElement).value,
    ).toBe('另一会话的职责'),
  );
  fireEvent.change(screen.getByRole('textbox', { name: '会话职责简介' }), {
    target: { value: '另一会话的新职责' },
  });
  fireEvent.click(screen.getByRole('button', { name: '保存简介' }));
  await waitFor(() =>
    expect(save).toHaveBeenCalledWith({
      session_id: 'ses_other',
      expected_revision: 0,
      description: '另一会话的新职责',
    }),
  );
});

test('manual regeneration is only a preview until explicitly adopted and saved', async () => {
  read.mockResolvedValue({
    ok: true,
    value: profile({ description_source: 'user', revision: 3, description: '用户保留的简介' }),
  });
  preview.mockResolvedValue({
    ok: true,
    value: profile({
      description_source: 'user',
      revision: 3,
      description: '用户保留的简介',
      generation: {
        state: 'completed',
        source_run_id: 'agt_source',
        source_turn_id: 'trn_source',
        model_profile_id: null,
        input_tokens: 17,
        output_tokens: 9,
        usage_complete: true,
        message: null,
        preview: { base_revision: 3, description: '新生成的建议' },
      },
    }),
  });
  render(<AgentProfileEditor sessionId="ses_main" visible />);
  const field = await screen.findByRole('textbox', { name: '会话职责简介' });
  fireEvent.click(screen.getByRole('button', { name: '重新生成建议' }));
  await screen.findByText('新生成的建议');
  expect((field as HTMLTextAreaElement).value).toBe('用户保留的简介');
  expect(save).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '采用为编辑内容' }));
  expect((field as HTMLTextAreaElement).value).toBe('新生成的建议');
  expect(save).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '保存简介' }));
  await waitFor(() =>
    expect(save).toHaveBeenCalledWith({
      session_id: 'ses_main',
      expected_revision: 3,
      description: '新生成的建议',
    }),
  );
});
