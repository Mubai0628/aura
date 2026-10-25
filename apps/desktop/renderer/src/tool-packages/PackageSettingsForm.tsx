import { useState } from 'react';
import type { CanonicalToolPackageToolV1, PackageToolSettingsV1 } from '@aura/contracts/generated';
import { endpointSlot, splitServiceUrl } from './package-settings';

interface Draft {
  configuration: string;
  url: string;
}

/** Instance configuration as JSON and user-supplied service addresses, per tool. */
export function SettingsForm({
  tools,
  settings,
  disabled,
  onChange,
}: {
  tools: CanonicalToolPackageToolV1[];
  settings: PackageToolSettingsV1[];
  disabled: boolean;
  onChange: (settings: PackageToolSettingsV1[], valid: boolean) => void;
}) {
  const needing = tools.filter(
    (tool) => tool.instance_configuration != null || endpointSlot(tool) !== undefined,
  );
  const [drafts, setDrafts] = useState<Record<string, Draft>>(() =>
    Object.fromEntries(
      needing.map((tool) => {
        const current = settings.find((entry) => entry.tool_key === tool.tool_key);
        const endpoint = current?.endpoints?.[0];
        return [
          tool.tool_key,
          {
            configuration:
              current?.configuration === undefined
                ? ''
                : JSON.stringify(current.configuration, null, 2),
            url: endpoint ? `${endpoint.origin}${endpoint.base_path}` : '',
          },
        ];
      }),
    ),
  );
  const [errors, setErrors] = useState<Record<string, string>>({});
  const update = (key: string, draft: Draft) => {
    const next = { ...drafts, [key]: draft };
    setDrafts(next);
    const nextErrors: Record<string, string> = {};
    const built: PackageToolSettingsV1[] = [];
    for (const tool of needing) {
      const value = next[tool.tool_key] ?? { configuration: '', url: '' };
      const entry: PackageToolSettingsV1 = { tool_key: tool.tool_key };
      if (tool.instance_configuration != null) {
        try {
          entry.configuration = JSON.parse(value.configuration) as unknown as NonNullable<
            PackageToolSettingsV1['configuration']
          >;
        } catch {
          nextErrors[tool.tool_key] = '配置需要是 JSON。';
        }
      }
      const slot = endpointSlot(tool);
      if (slot !== undefined) {
        const split = splitServiceUrl(value.url);
        if (split === undefined)
          nextErrors[tool.tool_key] = '服务地址需要是 https 开头的完整地址。';
        else entry.endpoints = [{ endpoint_slot_key: slot, ...split }];
      }
      built.push(entry);
    }
    setErrors(nextErrors);
    onChange(built, Object.keys(nextErrors).length === 0);
  };
  return (
    <fieldset className="package-settings" disabled={disabled}>
      <legend>工具设置</legend>
      <p>这些设置随验证一起运行；安装时使用的就是通过验证的这一份。</p>
      {needing.map((tool) => {
        const draft = drafts[tool.tool_key] ?? { configuration: '', url: '' };
        return (
          <div key={tool.tool_key}>
            <strong>{tool.ui.display_name}</strong>
            {tool.instance_configuration == null ? null : (
              <label>
                配置（JSON）
                <textarea
                  onChange={(event) =>
                    update(tool.tool_key, { ...draft, configuration: event.target.value })
                  }
                  rows={4}
                  value={draft.configuration}
                />
              </label>
            )}
            {endpointSlot(tool) === undefined ? null : (
              <label>
                服务地址
                <input
                  onChange={(event) => update(tool.tool_key, { ...draft, url: event.target.value })}
                  placeholder="https://api.example.com/v1"
                  value={draft.url}
                />
              </label>
            )}
            {errors[tool.tool_key] ? <p role="alert">{errors[tool.tool_key]}</p> : null}
          </div>
        );
      })}
    </fieldset>
  );
}
