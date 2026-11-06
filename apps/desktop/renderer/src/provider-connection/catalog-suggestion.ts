import type {
  ChatInstructionRole,
  ChatOutputTokenField,
  EndpointAuthScheme,
  ModelApiFormat,
  ProviderCatalogConnectionDefaultsView,
} from '@aura/contracts/generated';

/** What a new connection to a catalog Provider without a preset starts from: the address the
 *  catalog suggests and the protocol its package speaks, each unsaved until the user saves. */
export interface CatalogSuggestion {
  apiFormat: ModelApiFormat | '';
  authScheme: EndpointAuthScheme;
  baseUrl: string;
  chatInstructionRole: ChatInstructionRole | '';
  chatOutputTokenField: ChatOutputTokenField | '';
  addressSuggested: boolean;
  protocolSuggested: boolean;
}

export const catalogSuggestion = (
  defaults: ProviderCatalogConnectionDefaultsView | null | undefined,
): CatalogSuggestion => {
  const suggestion = defaults?.source === 'models_dev_api_suggestion' ? defaults : undefined;
  const protocol = suggestion?.protocol ?? undefined;
  return {
    apiFormat: protocol?.api_format ?? '',
    authScheme: protocol?.auth_scheme ?? 'none',
    baseUrl: suggestion?.base_url ?? '',
    chatInstructionRole: protocol?.chat_instruction_role ?? '',
    chatOutputTokenField: protocol?.chat_output_token_field ?? '',
    addressSuggested: suggestion !== undefined,
    protocolSuggested: protocol !== undefined,
  };
};
