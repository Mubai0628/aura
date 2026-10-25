import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react';

export interface McpSecretInputHandle {
  hasDraft(): boolean;
  /** The typed value as bytes for the secret channel; the field is cleared. */
  takeBytes(): Uint8Array | undefined;
}

/**
 * A password field whose value never enters React state: it is read once as bytes when the
 * form is saved and sent only on the secret channel.
 */
export const McpSecretInput = forwardRef<
  McpSecretInputHandle,
  { label: string; saved: boolean; disabled?: boolean }
>(function McpSecretInput({ label, saved, disabled = false }, forwarded) {
  const input = useRef<HTMLInputElement>(null);
  useImperativeHandle(
    forwarded,
    () => ({
      hasDraft: () => (input.current?.value.length ?? 0) > 0,
      takeBytes: () => {
        const value = input.current?.value;
        if (!value) return undefined;
        const bytes = new TextEncoder().encode(value);
        if (input.current) input.current.value = '';
        return bytes;
      },
    }),
    [],
  );
  useEffect(() => {
    const field = input.current;
    return () => {
      if (field) field.value = '';
    };
  }, []);
  return (
    <label>
      {label}
      <input
        ref={input}
        type="password"
        autoComplete="off"
        autoCapitalize="none"
        spellCheck={false}
        disabled={disabled}
        placeholder={saved ? '已设置；留空保持不变' : '只保存在 Windows 凭据管理器'}
      />
    </label>
  );
});
