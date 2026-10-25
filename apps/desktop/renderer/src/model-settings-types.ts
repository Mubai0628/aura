import type {
  ModelApiFormat,
  ModelEndpointConfig,
  ModelProfileConfig,
  ModelProviderConfig,
  ProviderCatalogConfigurationClass,
  ProviderCatalogConnectionDefaultsView,
} from '@aura/contracts/generated';

export interface ProviderCatalogItem {
  catalogModelCount: number;
  catalogProviderId?: string;
  configurationClass: ProviderCatalogConfigurationClass;
  connectionDefaults?: ProviderCatalogConnectionDefaultsView;
  configuredEndpointCount: number;
  id: string;
  initials: string;
  iconUrl?: string;
  localProvider: ModelProviderConfig | undefined;
  name: string;
  source: 'custom' | 'models.dev';
}

export interface ProviderConfigurationView extends ProviderCatalogItem {
  endpoints: ModelEndpointConfig[];
  models: ModelProfileConfig[];
}

export const ENDPOINT_API_TYPES: ReadonlyArray<{ label: string; value: ModelApiFormat }> = [
  { label: 'OpenAI Responses', value: 'open_ai_responses' },
  { label: 'OpenAI Chat Completions', value: 'open_ai_chat_completions' },
  { label: 'Anthropic Messages', value: 'anthropic_messages' },
];

export const endpointApiFormatLabel = (value: ModelApiFormat): string =>
  ENDPOINT_API_TYPES.find((option) => option.value === value)?.label ?? value;

export const providerInitials = (displayName: string): string => {
  const normalized = displayName.trim();
  if (normalized.length === 0) return '?';
  return Array.from(normalized).slice(0, 2).join('').toLocaleUpperCase('en-US');
};
