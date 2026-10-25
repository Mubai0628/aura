import { useRef, useState } from 'react';
import type { McpServerView } from '@aura/contracts/generated';
import { McpSecretInput, type McpSecretInputHandle } from './McpSecretInput';
import { mcpError } from './mcp-labels';

type Secret = { label: string; secretId: string };

/** The secrets a plugin's server names: its secret environment variables and sign-in secret. */
function secrets(server: McpServerView): Secret[] {
  const { transport } = server.config;
  if (transport.type === 'stdio')
    return (transport.secret_environment ?? []).map((entry) => ({
      label: entry.name,
      secretId: entry.secret_id,
    }));
  if (transport.auth?.type === 'secret')
    return [{ label: '登录密钥', secretId: transport.auth.secret_id }];
  return [];
}

/**
 * A plugin's server is edited with its plugin, so its secrets are entered here, each into
 * the credential the plugin keeps for it. A value goes only on the secret channel.
 */
export function McpPluginSecrets({
  server,
  locked,
  onSaved,
}: {
  server: McpServerView;
  locked: boolean;
  onSaved: (message: string, alert: boolean) => void;
}) {
  const inputs = useRef(new Map<string, McpSecretInputHandle | null>());
  const [saving, setSaving] = useState<string>();
  const needed = secrets(server);
  if (!needed.length) return null;
  const missing = server.problems.some((problem) => problem.includes('密钥'));
  const save = async (secret: Secret) => {
    const bytes = inputs.current.get(secret.secretId)?.takeBytes();
    if (!bytes) return;
    setSaving(secret.secretId);
    try {
      const result = await window.aura.v1.setMcpSecret(
        { server_id: server.config.id, secret_id: secret.secretId },
        bytes,
      );
      if (result.ok) onSaved(`已保存 ${secret.label}`, false);
      else onSaved(mcpError(result.error.message, '密钥没有确认保存，请重新填写。'), true);
    } finally {
      setSaving(undefined);
    }
  };
  return (
    <div className="mcp-plugin-secrets" aria-label={`${server.config.display_name} 的密钥`}>
      {needed.map((secret) => (
        <div key={secret.secretId} className="mcp-plugin-secret">
          <McpSecretInput
            ref={(handle) => {
              inputs.current.set(secret.secretId, handle);
            }}
            label={secret.label}
            saved={!missing}
            disabled={locked || saving !== undefined}
          />
          <button
            type="button"
            disabled={locked || saving !== undefined}
            onClick={() => void save(secret)}
          >
            保存
          </button>
        </div>
      ))}
    </div>
  );
}
