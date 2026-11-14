import { act, fireEvent, render, screen, waitFor, cleanup } from '@testing-library/react';
import type { TerminalSensitivePendingV1 } from '@aura/contracts/generated';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { toolTabs } from '../right-workspace/tool-tabs';
import { SensitiveInputForm, SensitiveInputPanel } from './SensitiveInputPanel';
const request = {
  session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  terminal_id: 'term_test',
  tool_call_id: 'tcl_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  prompt: '请输入临时测试值',
  expires_at: '2026-10-14T14:00:00Z',
};
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
describe('terminal sensitive input', () => {
  it('uses a password field, sends bytes outside metadata and clears DOM and bytes', async () => {
    let source: Uint8Array | undefined;
    const submitSensitive = vi.fn(async (metadata: unknown, bytes: Uint8Array) => {
      source = bytes;
      expect(metadata).toEqual({
        session_id: request.session_id,
        tool_call_id: request.tool_call_id,
      });
      expect(new TextDecoder().decode(bytes)).toBe('temporary-only');
      expect((screen.getByLabelText('安全输入') as HTMLInputElement).value).toBe('');
      return { ok: true, value: { status: 'completed' } };
    });
    vi.stubGlobal('aura', { v1: { terminal: { submitSensitive } } });
    render(<SensitiveInputForm request={request} />);
    expect(screen.getByLabelText('安全输入').getAttribute('type')).toBe('password');
    fireEvent.change(screen.getByLabelText('安全输入'), { target: { value: 'temporary-only' } });
    fireEvent.click(screen.getByRole('button', { name: '提交' }));
    await waitFor(() => expect(submitSensitive).toHaveBeenCalledOnce());
    await waitFor(() => expect(source?.every((byte) => byte === 0)).toBe(true));
    expect(screen.queryByText('temporary-only')).toBeNull();
  });
  it('says nothing was typed when the request has ended, and to look first when it was not confirmed', async () => {
    const submitSensitive = vi
      .fn()
      .mockResolvedValueOnce({
        ok: false,
        error: {
          code: 'terminal.sensitive.expired_or_consumed',
          message: 'terminal.sensitive.expired_or_consumed',
        },
      })
      .mockRejectedValueOnce(new Error('ipc closed'));
    vi.stubGlobal('aura', { v1: { terminal: { submitSensitive } } });
    render(<SensitiveInputForm request={request} />);
    fireEvent.change(screen.getByLabelText('安全输入'), { target: { value: 'temporary-only' } });
    fireEvent.click(screen.getByRole('button', { name: '提交' }));
    expect((await screen.findByRole('alert')).textContent).toBe(
      '这次安全输入请求已经结束（超时或已经用过），没有输入任何内容。请查看终端，需要时让模型再请求一次。',
    );
    fireEvent.change(screen.getByLabelText('安全输入'), { target: { value: 'temporary-only' } });
    fireEvent.click(screen.getByRole('button', { name: '提交' }));
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe(
        '没能确认这次输入是否送到终端。请先看终端当前状态，确认没收到再输入一次，避免重复输入。',
      ),
    );
  });

  it('cancellation sends metadata only and discards unsent input', async () => {
    const cancelSensitive = vi.fn(async () => ({ ok: true, value: { status: 'cancelled' } }));
    const submitSensitive = vi.fn();
    vi.stubGlobal('aura', { v1: { terminal: { cancelSensitive, submitSensitive } } });
    const view = render(<SensitiveInputForm request={request} />);
    const input = screen.getByLabelText('安全输入') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'discard-test-value' } });
    fireEvent.click(screen.getByRole('button', { name: '取消' }));
    await waitFor(() =>
      expect(cancelSensitive).toHaveBeenCalledWith({
        session_id: request.session_id,
        tool_call_id: request.tool_call_id,
      }),
    );
    expect(submitSensitive).not.toHaveBeenCalled();
    expect(input.value).toBe('');
    fireEvent.change(input, { target: { value: 'unmount-value' } });
    view.unmount();
    expect(input.value).toBe('');
  });
  it('says nothing was typed once the prompt has gone, after Core ends the request, and shows the terminal', async () => {
    let listener: ((pending: TerminalSensitivePendingV1) => void) | undefined;
    let answer: ((value: unknown) => void) | undefined;
    let source: Uint8Array | undefined;
    const submitSensitive = vi.fn(
      (_metadata: unknown, bytes: Uint8Array) =>
        new Promise((resolve) => {
          source = bytes;
          answer = resolve;
        }),
    );
    const pendingSensitive = vi.fn(async () => ({
      ok: true,
      value: { session_id: request.session_id, requests: [request] },
    }));
    const onSensitive = vi.fn((next: (pending: TerminalSensitivePendingV1) => void) => {
      listener = next;
      return () => undefined;
    });
    vi.stubGlobal('aura', { v1: { terminal: { submitSensitive, pendingSensitive, onSensitive } } });
    const reveal = vi.spyOn(toolTabs, 'revealTerminal').mockImplementation(() => undefined);
    render(<SensitiveInputPanel sessionId={request.session_id} />);
    fireEvent.change(await screen.findByLabelText('安全输入'), {
      target: { value: 'late-value' },
    });
    fireEvent.click(screen.getByRole('button', { name: '提交' }));
    await waitFor(() => expect(submitSensitive).toHaveBeenCalledOnce());
    // Core ends the request before its answer arrives.
    pendingSensitive.mockResolvedValue({
      ok: true,
      value: { session_id: request.session_id, requests: [] },
    });
    act(() => listener?.({ session_id: request.session_id, requests: [] }));
    expect(screen.queryByRole('form', { name: '终端安全输入' })).toBeNull();
    await act(async () =>
      answer?.({
        ok: false,
        error: {
          code: 'terminal.sensitive.prompt_gone',
          category: 'conflict',
          message: '终端已不在等这次输入，没有输入。',
          retryable: false,
          correlation_id: 'req_01ARZ3NDEKTSV4RRFFQ69G5FAV',
          terminal: 'failed',
          details: {},
        },
      }),
    );
    const notice = await screen.findByRole('region', { name: '终端安全输入' });
    expect(notice.textContent).toContain(
      '终端已不在等这次输入，没有输入。请查看终端，需要时让模型再请求一次。',
    );
    expect(notice.textContent).not.toContain('late-value');
    expect(source?.every((byte) => byte === 0)).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: '查看终端' }));
    expect(reveal).toHaveBeenCalledWith(request.session_id, request.terminal_id);
    fireEvent.click(screen.getByRole('button', { name: '关闭' }));
    expect(screen.queryByRole('region', { name: '终端安全输入' })).toBeNull();
    reveal.mockRestore();
  });
});
