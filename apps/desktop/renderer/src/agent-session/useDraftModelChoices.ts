import type { ModelSettingsProjection } from '@aura/contracts/generated';
import { useEffect, useMemo, useState } from 'react';
import { useScopedModelProjection } from '../model-projection-store';
import type { ModelChoice } from '../ModelPicker';

export function useDraftModelChoices() {
  const route = useMemo(
    () => ({ projection_kind: 'model_settings', projection_scope: { type: 'global' } }) as const,
    [],
  );
  const live = useScopedModelProjection(route);
  const [snapshot, setSnapshot] = useState<ModelSettingsProjection>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    let active = true;
    void window.aura.v1
      .getModelSettings()
      .then((result) => {
        if (!active) return;
        if (result.ok) {
          setSnapshot(result.value);
          setError(undefined);
        } else setError('无法读取模型配置，请检查连接。');
      })
      .catch(() => {
        if (active) setError('无法读取模型配置，请检查连接。');
      });
    return () => {
      active = false;
    };
  }, []);
  const settings = live?.payload ?? snapshot;
  const choices: ModelChoice[] = [];
  if (settings)
    for (const providerId of settings.registry.provider_order) {
      const provider = settings.registry.providers[providerId];
      if (!provider) continue;
      for (const endpointId of provider.endpoint_order) {
        const endpoint = settings.registry.endpoints[endpointId];
        if (!endpoint) continue;
        for (const modelId of endpoint.model_order) {
          const model = settings.registry.models[modelId];
          if (model)
            choices.push({
              id: model.id,
              label: model.display_name,
              provider: provider.display_name,
              detail: endpoint.display_name,
              reasoning: {
                levels:
                  settings.reasoning_efforts?.find((entry) => entry.model_profile_id === model.id)
                    ?.reasoning_efforts ?? [],
                level: model.request_defaults.reasoning_effort ?? undefined,
              },
              ...(!model.enabled ? { disabledReason: '模型已停用' } : {}),
            });
        }
      }
    }
  // Only a prospective choice: Core validates credentials/readiness when binding the new Session.
  return {
    choices,
    loading: !settings && !error,
    error: settings ? undefined : error,
    defaultId: settings?.default_model_profile_id ?? undefined,
  };
}
