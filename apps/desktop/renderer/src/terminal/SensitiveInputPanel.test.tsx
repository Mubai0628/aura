import { fireEvent, render, screen, waitFor, cleanup } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { SensitiveInputForm } from './SensitiveInputPanel';
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
});
