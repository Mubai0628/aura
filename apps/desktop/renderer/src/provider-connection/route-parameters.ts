import type {
  ModelEndpointConfig,
  ProviderPresetParameterView,
  ProviderPresetVariantView,
} from '@aura/contracts/generated';
import { useState } from 'react';

import { fillBaseUrl, matchBaseUrl, templateParameters } from './base-url-template';

interface RouteParametersInput {
  /** Every route still to connect is chosen, each to be saved at the address the values make. */
  allRoutesSelected: boolean;
  /** The Base URL field, which an address that takes no values is. */
  baseUrl: string;
  /** The chosen preset; none while every route still to connect is chosen. */
  selectedPreset: ProviderPresetVariantView | undefined;
  unconnectedVariants: readonly ProviderPresetVariantView[];
}

/** The values of the user's own that the chosen routes' addresses take, and the address they
 *  make, which is where the key goes. */
export function useRouteParameters({
  allRoutesSelected,
  baseUrl,
  selectedPreset,
  unconnectedVariants,
}: RouteParametersInput) {
  const [parameterValues, setParameterValues] = useState<Record<string, string>>({});
  // Every route's still to connect, each asked once, or the chosen preset's.
  const urlParameters: readonly ProviderPresetParameterView[] = allRoutesSelected
    ? unconnectedVariants
        .flatMap(templateParameters)
        .filter(
          (parameter, index, all) =>
            all.findIndex((other) => other.name === parameter.name) === index,
        )
    : selectedPreset === undefined
      ? []
      : templateParameters(selectedPreset);
  const effectiveBaseUrl =
    selectedPreset !== undefined && templateParameters(selectedPreset).length > 0
      ? (fillBaseUrl(selectedPreset, parameterValues) ?? selectedPreset.base_url)
      : baseUrl;

  /** A host label is the same in any case, so it is kept as the URL parser gives it. */
  const setUrlParameterValue = (parameter: ProviderPresetParameterView, value: string) => {
    const kept = parameter.kind === 'domain_label' ? value.toLowerCase() : value;
    setParameterValues((current) => ({ ...current, [parameter.name]: kept }));
  };

  /** The values `endpoint`'s address was made with from `preset`; none for a new connection. */
  const readParameterValues = (
    preset?: ProviderPresetVariantView,
    endpoint?: ModelEndpointConfig,
  ) => {
    setParameterValues(
      preset === undefined || endpoint === undefined
        ? {}
        : (matchBaseUrl(preset, endpoint.base_url) ?? {}),
    );
  };

  /** Each route still to connect at the address the values make; none while a value is missing
   *  or invalid, which the editor's checks name before a save. */
  const filledVariants = (): ProviderPresetVariantView[] | undefined => {
    const variants: ProviderPresetVariantView[] = [];
    for (const variant of unconnectedVariants) {
      const filled = fillBaseUrl(variant, parameterValues);
      if (filled === undefined) return undefined;
      variants.push({ ...variant, base_url: filled });
    }
    return variants;
  };

  return {
    effectiveBaseUrl,
    filledVariants,
    parameterValues,
    readParameterValues,
    setUrlParameterValue,
    urlParameters,
  };
}
