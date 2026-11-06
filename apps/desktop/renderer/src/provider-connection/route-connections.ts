import type {
  IpcErrorEnvelope,
  ModelEndpointCreateRequest,
  ModelEndpointTimeouts,
  ProviderPresetVariantView,
  Revision,
} from '@aura/contracts/generated';

export interface RouteConnectionsInput {
  variants: readonly ProviderPresetVariantView[];
  providerId: string;
  expectedRevision: Revision;
  timeouts: ModelEndpointTimeouts;
  maxParallelModelRequests: number;
  /** One API key for every route, or none. Each route's connection is sent its own copy, which
   *  the bridge zeroes; this one is left for the caller to zero. */
  secret: Uint8Array | undefined;
}

export interface RouteConnectionsResult {
  /** The names of the connections saved, in order. */
  created: string[];
  /** The route whose connection was not saved, and why; the routes after it were not tried. */
  failure?: { variant: ProviderPresetVariantView; error: IpcErrorEnvelope };
}

/** One connection for each of the variants, one after another, each at the revision the one
 *  before left. Each is saved on its own: a failure keeps those already saved. */
export async function createRouteConnections(
  input: RouteConnectionsInput,
): Promise<RouteConnectionsResult> {
  let revision = input.expectedRevision;
  const created: string[] = [];
  for (const variant of input.variants) {
    const request: ModelEndpointCreateRequest = {
      anthropic_workspace_id: null,
      api_format: variant.api_format,
      auth_scheme: variant.auth_scheme,
      base_url: variant.base_url,
      chat_instruction_role: variant.chat_instruction_role ?? null,
      chat_output_token_field: variant.chat_output_token_field ?? null,
      discovery_mode: variant.discovery_mode,
      display_name: variant.display_name,
      expected_revision: revision,
      max_parallel_model_requests: input.maxParallelModelRequests,
      openai_organization_id: null,
      openai_project_id: null,
      provider_id: input.providerId,
      timeouts: input.timeouts,
    };
    if (input.secret === undefined) {
      const result = await window.aura.v1.createModelEndpoint(request);
      if (!result.ok) return { created, failure: { error: result.error, variant } };
      revision = result.value.revision;
    } else {
      const copy = input.secret.slice();
      try {
        const result = await window.aura.v1.createModelEndpointWithCredential(request, copy);
        if (!result.ok) return { created, failure: { error: result.error, variant } };
        revision = result.value.models_revision;
      } finally {
        copy.fill(0);
      }
    }
    created.push(variant.display_name);
  }
  return { created };
}
