import type { EndpointCredentialStatus } from '@aura/contracts/generated';
import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react';

export interface SecretApiKeyInputHandle {
  clear(): void;
  hasDraft(): boolean;
  takeBytes(): Uint8Array | undefined;
}

interface SecretApiKeyInputProps {
  disabled?: boolean;
  onDraftStateChange?: (hasDraft: boolean) => void;
  status: EndpointCredentialStatus | undefined;
}

export const SecretApiKeyInput = forwardRef<SecretApiKeyInputHandle, SecretApiKeyInputProps>(
  function SecretApiKeyInput({ disabled = false, onDraftStateChange, status }, forwardedRef) {
    const inputRef = useRef<HTMLInputElement>(null);
    const [whitespaceWarning, setWhitespaceWarning] = useState(false);
    const saved = status?.state === 'configured';

    const clear = useCallback(() => {
      if (inputRef.current !== null) inputRef.current.value = '';
      setWhitespaceWarning(false);
      onDraftStateChange?.(false);
    }, [onDraftStateChange]);

    useImperativeHandle(
      forwardedRef,
      () => ({
        clear,
        hasDraft: () => inputRef.current?.value.length !== 0,
        takeBytes: () => {
          const value = inputRef.current?.value;
          if (value === undefined || value.length === 0) return undefined;
          const bytes = new TextEncoder().encode(value);
          clear();
          return bytes;
        },
      }),
      [clear],
    );

    useEffect(() => {
      const input = inputRef.current;
      return () => {
        if (input !== null) {
          input.value = '';
        }
      };
    }, []);

    const updateDraftFacts = () => {
      const value = inputRef.current?.value ?? '';
      onDraftStateChange?.(value.length > 0);
      setWhitespaceWarning(value.startsWith(' ') || value.endsWith(' '));
    };

    return (
      <div className="secret-api-key-field">
        <label htmlFor="model-endpoint-api-key">
          <span>API Key</span>
        </label>
        <div className="secret-api-key-control">
          <input
            aria-describedby={whitespaceWarning ? 'api-key-whitespace-warning' : undefined}
            autoCapitalize="none"
            autoComplete="off"
            disabled={disabled}
            id="model-endpoint-api-key"
            onInput={updateDraftFacts}
            placeholder={saved ? '••••••••••••••••' : '输入后通过专用 SecretInput 通道提交'}
            ref={inputRef}
            spellCheck={false}
            type="password"
          />
        </div>
        {whitespaceWarning ? (
          <p className="field-warning" id="api-key-whitespace-warning">
            API Key 含首尾空格；Aura 会按原始 UTF-8 字节保存，不会自动裁剪。
          </p>
        ) : null}
      </div>
    );
  },
);
