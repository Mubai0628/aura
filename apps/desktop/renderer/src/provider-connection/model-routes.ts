import type {
  ModelEndpointConfig,
  ProviderCatalogConnectionDefaultsView,
  ProviderCatalogModelRouteView,
  ProviderPresetVariantView,
} from '@aura/contracts/generated';

import { matchBaseUrl } from './base-url-template';

/** The editor's choice of every variant still to connect, one connection each. */
export const ALL_ROUTES_VARIANT = '*';

/** The preset variants of a Provider whose models each go by their own; none for any other. */
export function byModelVariants(
  defaults: ProviderCatalogConnectionDefaultsView | null | undefined,
): readonly ProviderPresetVariantView[] {
  return defaults?.source === 'aura_preset' && defaults.by_model === true ? defaults.variants : [];
}

/** Whether a connection is the variant's route: the same protocol, at the variant's address
 *  with any values of the user's own it takes. */
export function connectionIsVariant(
  variant: ProviderPresetVariantView,
  endpoint: Pick<ModelEndpointConfig, 'api_format' | 'base_url'>,
): boolean {
  return (
    variant.api_format === endpoint.api_format &&
    matchBaseUrl(variant, endpoint.base_url) !== undefined
  );
}

/** The variants no connection is yet the route of. */
export function missingVariants(
  variants: readonly ProviderPresetVariantView[],
  endpoints: readonly Pick<ModelEndpointConfig, 'api_format' | 'base_url'>[],
): ProviderPresetVariantView[] {
  return variants.filter(
    (variant) => !endpoints.some((endpoint) => connectionIsVariant(variant, endpoint)),
  );
}

/** Where a catalog model of a Provider whose models go by their own variants is added. */
export type ModelTarget =
  | { kind: 'endpoint'; endpoint: ModelEndpointConfig }
  | { kind: 'unconnected'; variant: ProviderPresetVariantView }
  | { kind: 'unsupported' };

/** The first connection of the model's variant, in the order they were created. */
export function modelTarget(
  route: ProviderCatalogModelRouteView | null | undefined,
  variants: readonly ProviderPresetVariantView[],
  endpoints: readonly ModelEndpointConfig[],
): ModelTarget {
  if (route?.type !== 'variant') return { kind: 'unsupported' };
  const variant = variants.find((item) => item.variant_id === route.variant_id);
  if (variant === undefined) return { kind: 'unsupported' };
  const endpoint = endpoints.find((item) => connectionIsVariant(variant, item));
  return endpoint === undefined ? { kind: 'unconnected', variant } : { kind: 'endpoint', endpoint };
}
