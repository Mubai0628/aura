import type { ModelSettingsProjection, ProviderCatalogProjection } from '@aura/contracts/generated';
import { useEffect, useMemo, useSyncExternalStore } from 'react';

import type { ResultEnvelope } from '../../shared/aura-api';
import { useScopedModelProjection } from './model-projection-store';

interface ModelSettingsState {
  catalog: ProviderCatalogProjection | undefined;
  catalogError: string | undefined;
  error: string | undefined;
  loading: boolean;
  settings: ModelSettingsProjection | undefined;
  writeUnavailableReason: string | undefined;
}

let state: ModelSettingsState = {
  catalog: undefined,
  catalogError: undefined,
  error: undefined,
  loading: true,
  settings: undefined,
  writeUnavailableReason: undefined,
};
const listeners = new Set<() => void>();

const publish = (next: ModelSettingsState) => {
  state = next;
  for (const listener of listeners) listener();
};

const patchState = (patch: Partial<ModelSettingsState>) => publish({ ...state, ...patch });
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
const getSnapshot = () => state;

const applySettingsResult = (settingsResult: ResultEnvelope<ModelSettingsProjection>) => {
  const settingsPatch: Partial<ModelSettingsState> = settingsResult.ok
    ? {
        error: undefined,
        settings: settingsResult.value,
        writeUnavailableReason: undefined,
      }
    : state.settings === undefined
      ? { error: settingsResult.error.message }
      : {
          writeUnavailableReason:
            'Aura Core 正在重新连接；现有配置仍可查看，恢复连接后才能保存更改。',
        };
  publish({
    ...state,
    ...settingsPatch,
    loading: false,
  });
};

const applyCatalogResult = (catalogResult: ResultEnvelope<ProviderCatalogProjection>) => {
  patchState(
    catalogResult.ok
      ? { catalog: catalogResult.value, catalogError: undefined }
      : { catalogError: catalogResult.error.message },
  );
};

const refresh = async () => {
  patchState({
    ...(state.settings === undefined ? { loading: true } : {}),
    error: undefined,
  });
  try {
    // Catalog freshness is a live network/LKG probe. Settings heading and
    // Core-owned configuration must not wait for that probe to settle.
    await Promise.all([
      window.aura.v1.getModelSettings().then(applySettingsResult),
      window.aura.v1.ensureModelProviderCatalogFresh().then(applyCatalogResult),
    ]);
  } catch (error) {
    const message = error instanceof Error ? error.message : '模型设置读取失败。';
    publish({
      ...state,
      error: state.settings === undefined ? message : undefined,
      loading: false,
      writeUnavailableReason:
        state.settings === undefined
          ? state.writeUnavailableReason
          : 'Aura Core 正在重新连接；现有配置仍可查看，恢复连接后才能保存更改。',
    });
  }
};

const refreshCatalog = async () => {
  try {
    const result = await window.aura.v1.refreshModelProviderCatalog();
    patchState(
      result.ok
        ? { catalog: result.value, catalogError: undefined }
        : { catalogError: result.error.message },
    );
  } catch (error) {
    patchState({
      catalogError: error instanceof Error ? error.message : 'models.dev 目录刷新失败。',
    });
  }
};

export function useModelSettingsControl() {
  const current = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  const settingsRoute = useMemo(
    () =>
      ({
        projection_kind: 'model_settings',
        projection_scope: { type: 'global' },
      }) as const,
    [],
  );
  const catalogRoute = useMemo(
    () =>
      ({
        projection_kind: 'provider_catalog',
        projection_scope: { type: 'global' },
      }) as const,
    [],
  );
  const settingsChange = useScopedModelProjection(settingsRoute);
  const catalogChange = useScopedModelProjection(catalogRoute);

  useEffect(() => {
    void refresh();
  }, []);

  useEffect(() => {
    if (settingsChange === undefined) return;
    patchState({
      error: undefined,
      loading: false,
      settings: settingsChange.payload,
      writeUnavailableReason: undefined,
    });
  }, [settingsChange]);

  useEffect(() => {
    if (catalogChange === undefined) return;
    patchState({ catalog: catalogChange.payload, catalogError: undefined });
  }, [catalogChange]);

  return {
    ...current,
    refresh,
    refreshCatalog,
  };
}

export const resetModelSettingsStoreForTests = () => {
  state = {
    catalog: undefined,
    catalogError: undefined,
    error: undefined,
    loading: true,
    settings: undefined,
    writeUnavailableReason: undefined,
  };
  listeners.clear();
};
