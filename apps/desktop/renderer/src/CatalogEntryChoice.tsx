import type {
  ModelCatalogEntryView,
  ModelProfileCatalogEntriesView,
  ModelProfileConfig,
  ModelSettingsProjection,
  ProviderCatalogMatchCandidate,
} from '@aura/contracts/generated';
import { useEffect, useState } from 'react';

import { userMessage } from './agent-session/user-message';
import type { NotificationInput } from './notification-center-store';
import { MODEL_FAILURE_FALLBACK } from './provider-connection/notices';

const entryKey = (entry: ProviderCatalogMatchCandidate) =>
  `${entry.catalog_provider_id}/${entry.catalog_model_id}`;

/**
 * The catalog entries a model's name matches, when several Providers list it: choosing one links
 * the model to it, with that entry's context window, thinking levels and speed modes. Core lists
 * the entries; the choice is written at once.
 */
export function CatalogEntryChoice({
  disabled,
  onNotify,
  onSaved,
  profile,
  settings,
}: {
  disabled: boolean;
  onNotify: (notification: NotificationInput) => void;
  onSaved: () => Promise<void>;
  profile: ModelProfileConfig;
  settings: ModelSettingsProjection;
}) {
  const [view, setView] = useState<ModelProfileCatalogEntriesView>();
  const [busy, setBusy] = useState(false);
  const linked = profile.catalog ? entryKey(profile.catalog) : undefined;
  useEffect(() => {
    let current = true;
    // Without the list there is nothing to choose from; the rest of the dialog works as before.
    Promise.resolve()
      .then(() => window.aura.v1.getModelProfileCatalogEntries({ model_profile_id: profile.id }))
      .then(
        (result) => {
          if (current && result.ok) setView(result.value);
        },
        () => undefined,
      );
    return () => {
      current = false;
    };
  }, [profile.id, linked]);
  if (view === undefined || view.entries.length < 2) return null;
  const label = (entry: ModelCatalogEntryView) => {
    const named = view.entries.filter(
      (each) => each.provider_display_name === entry.provider_display_name,
    );
    const provider = named.some((each) => each.catalog_provider_id !== entry.catalog_provider_id)
      ? `${entry.provider_display_name}（${entry.catalog_provider_id}）`
      : entry.provider_display_name;
    return entry.catalog_model_id === profile.provider_model_name
      ? provider
      : `${provider} · ${entry.catalog_model_id}`;
  };
  const choose = async (key: string) => {
    const entry = view.entries.find((each) => entryKey(each) === key);
    if (entry === undefined || busy) return;
    setBusy(true);
    try {
      const result = await window.aura.v1.applyModelProfileCatalogUpdate({
        catalog_entry: {
          catalog_model_id: entry.catalog_model_id,
          catalog_provider_id: entry.catalog_provider_id,
        },
        expected_catalog_object_hash: entry.catalog_object_hash,
        expected_catalog_revision: view.catalog_revision,
        expected_model_profile_revision: profile.revision,
        expected_models_revision: settings.models_revision,
        model_profile_id: profile.id,
      });
      if (!result.ok) {
        onNotify({
          dedupeKey: `目录条目未保存:${result.error.code}`,
          message: userMessage(result.error, MODEL_FAILURE_FALLBACK),
          title: '目录条目未保存',
          tone: 'danger',
        });
        return;
      }
      await onSaved();
    } finally {
      setBusy(false);
    }
  };
  return (
    <label>
      <span>目录条目</span>
      <select
        disabled={disabled || busy}
        onChange={(event) => void choose(event.target.value)}
        value={view.chosen ? entryKey(view.chosen) : ''}
      >
        {view.chosen ? null : (
          <option disabled value="">
            未选择
          </option>
        )}
        {view.entries.map((entry) => (
          <option key={entryKey(entry)} value={entryKey(entry)}>
            {label(entry)}
          </option>
        ))}
      </select>
    </label>
  );
}
