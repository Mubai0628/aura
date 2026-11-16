import { useState } from 'react';

const KEY = 'aura.ui.v1.settings-display';

interface SettingsDisplayPreferences {
  configuredOnlyProviders: string[];
  modelConnectionByProvider: Record<string, string>;
  modelProviderId: string | undefined;
  toolFamily: string;
}

function fields(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function readPreferences(): SettingsDisplayPreferences {
  let stored: Record<string, unknown> = {};
  try {
    stored = fields(JSON.parse(window.localStorage.getItem(KEY) ?? '{}'));
  } catch {
    // A missing or unreadable profile starts with the default display.
  }
  return {
    configuredOnlyProviders: Array.isArray(stored.configuredOnlyProviders)
      ? stored.configuredOnlyProviders.filter((id): id is string => typeof id === 'string')
      : [],
    modelConnectionByProvider: Object.fromEntries(
      Object.entries(fields(stored.modelConnectionByProvider)).filter(
        (entry): entry is [string, string] => typeof entry[1] === 'string',
      ),
    ),
    modelProviderId:
      typeof stored.modelProviderId === 'string' ? stored.modelProviderId : undefined,
    toolFamily: typeof stored.toolFamily === 'string' ? stored.toolFamily : 'all',
  };
}

/** Store display choices when the user changes them, in the desktop UI profile. */
export function useSettingsDisplayPreferences() {
  const [preferences, setPreferences] = useState(readPreferences);
  const save = (patch: Partial<SettingsDisplayPreferences>) => {
    const next = { ...preferences, ...patch };
    try {
      window.localStorage.setItem(KEY, JSON.stringify(next));
    } catch {
      // Keep the display usable when the UI profile storage is unavailable.
    }
    setPreferences(next);
  };
  return [preferences, save] as const;
}
