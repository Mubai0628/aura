import type {
  ModelEndpointConfig,
  ModelProfileConfig,
  ModelSettingsProjection,
} from '@aura/contracts/generated';
import { useEffect, useMemo, useState } from 'react';
import { useScopedModelProjection } from '../model-projection-store';
import { profileStatusLabel } from '../model-profile-status';
import type { ModelChoice } from '../ModelPicker';
import { credentialStatusLabel } from '../provider-connection/credential-status';
import { speedModesOf } from '../speed-mode';
import { failedAction } from './failure-text';

const LIST_UNREAD = '模型列表没有读到';
const LIST_NEXT = '暂时不能选模型；关闭再打开这个列表会重新读取。';

/**
 * Why a new conversation cannot use the model yet, by the rule Core applies when it binds one: the
 * model is on, has an output limit, and its connection needs no credential or has a configured one.
 */
function notReady(
  settings: ModelSettingsProjection,
  model: ModelProfileConfig,
  endpoint: ModelEndpointConfig,
): string | undefined {
  if (!model.enabled) return '模型已停用';
  const profile = profileStatusLabel(model);
  if (profile !== '已配置') return profile;
  if (endpoint.auth_scheme === 'none') return undefined;
  const credential = settings.active_credentials.find((item) => item.endpoint_id === endpoint.id);
  return credential?.state === 'configured'
    ? undefined
    : credentialStatusLabel(endpoint, credential, undefined);
}

export function useDraftModelChoices() {
  const route = useMemo(
    () => ({ projection_kind: 'model_settings', projection_scope: { type: 'global' } }) as const,
    [],
  );
  const live = useScopedModelProjection(route);
  const [snapshot, setSnapshot] = useState<ModelSettingsProjection>();
  const [error, setError] = useState<string>();
  // Opening the list again after a failed read reads it again.
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    void window.aura.v1
      .getModelSettings()
      .then((result) => {
        if (!active) return;
        if (result.ok) {
          setSnapshot(result.value);
          setError(undefined);
        } else setError(failedAction(result.error, LIST_UNREAD, LIST_NEXT));
      })
      .catch((reason: unknown) => {
        if (active) setError(failedAction(reason, LIST_UNREAD, LIST_NEXT));
      });
    return () => {
      active = false;
    };
  }, [attempt]);
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
          const reason = model ? notReady(settings, model, endpoint) : undefined;
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
              speedModes: model.request_defaults.allow_speed_modes
                ? (speedModesOf(settings, model.id)?.modes.map((mode) => mode.name) ?? [])
                : [],
              ...(reason ? { disabledReason: reason } : {}),
            });
        }
      }
    }
  // Only a prospective choice: Core validates credentials/readiness again when binding the new Session.
  return {
    choices,
    loading: !settings && !error,
    error: settings ? undefined : error,
    defaultId: settings?.default_model_profile_id ?? undefined,
    retry: () => {
      setError(undefined);
      setAttempt((count) => count + 1);
    },
  };
}
