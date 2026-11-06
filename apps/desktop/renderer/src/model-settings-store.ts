import type { ModelSettingsProjection, ProviderCatalogProjection } from '@aura/contracts/generated';
import { useEffect, useMemo, useSyncExternalStore } from 'react';

import type { ResultEnvelope } from '../../shared/aura-api';
import { useScopedModelProjection } from './model-projection-store';

/** Core refused a request because another change to the same settings was still being written. */
export const FACTS_CHANGED = 'broker.facts_changed';
export const MODEL_SETTINGS_CHANGING = '模型设置正在被修改，请稍后再试。';
const CORE_RECONNECTING = 'Aura Core 正在重新连接；现有配置仍可查看，恢复连接后才能保存更改。';
// Core has already waited for that change; the settings are read again a little after it.
const CHANGING_REREAD_MS = 1_000;
const CHANGING_REREADS = 3;
let changingRereads = 0;
let changingReread: number | undefined;

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
  const changing = !settingsResult.ok && settingsResult.error.code === FACTS_CHANGED;
  const settingsPatch: Partial<ModelSettingsState> = settingsResult.ok
    ? {
        error: undefined,
        settings: settingsResult.value,
        writeUnavailableReason: undefined,
      }
    : state.settings === undefined
      ? { error: changing ? MODEL_SETTINGS_CHANGING : settingsResult.error.message }
      : { writeUnavailableReason: changing ? MODEL_SETTINGS_CHANGING : CORE_RECONNECTING };
  publish({
    ...state,
    ...settingsPatch,
    loading: false,
  });
  if (!changing) {
    changingRereads = 0;
  } else if (changingReread === undefined && changingRereads < CHANGING_REREADS) {
    changingRereads += 1;
    changingReread = window.setTimeout(() => {
      changingReread = undefined;
      void window.aura.v1.getModelSettings().then(applySettingsResult, () => undefined);
    }, CHANGING_REREAD_MS);
  }
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
        state.settings === undefined ? state.writeUnavailableReason : CORE_RECONNECTING,
    });
  }
};

/** Refreshes the models.dev catalog; what went wrong when it was not refreshed. */
const refreshCatalog = async (): Promise<{ code: string; message: string } | undefined> => {
  try {
    const result = await window.aura.v1.refreshModelProviderCatalog();
    patchState(
      result.ok
        ? { catalog: result.value, catalogError: undefined }
        : { catalogError: result.error.message },
    );
    if (!result.ok) return result.error;
    if (result.value.state === 'unavailable') {
      return {
        code: 'catalog.unavailable',
        message: 'models.dev 目录暂不可用，请检查网络后重试。',
      };
    }
    return result.value.state === 'refresh_failed_with_lkg'
      ? { code: 'catalog.refresh_failed', message: 'models.dev 目录没有刷新，正在使用上次的目录。' }
      : undefined;
  } catch (error) {
    const message = error instanceof Error ? error.message : 'models.dev 目录刷新失败。';
    patchState({ catalogError: message });
    return { code: 'catalog.refresh_failed', message };
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
  if (changingReread !== undefined) window.clearTimeout(changingReread);
  changingReread = undefined;
  changingRereads = 0;
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
