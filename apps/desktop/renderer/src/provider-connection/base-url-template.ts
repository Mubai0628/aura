import type {
  ProviderPresetParameterKind,
  ProviderPresetParameterView,
  ProviderPresetVariantView,
} from '@aura/contracts/generated';

/** A preset variant's address: a base URL with a `{name}` for each value of the user's own. */
export type BaseUrlTemplate = Pick<ProviderPresetVariantView, 'base_url' | 'base_url_parameters'>;

const LONGEST_LABEL = 63;
const LONGEST_SEGMENT = 128;

export const templateParameters = (
  template: BaseUrlTemplate,
): readonly ProviderPresetParameterView[] => template.base_url_parameters ?? [];

/** Whether `value` may stand for a parameter of `kind`: one whole label of the host, or one
 *  whole segment of the path, as Core decides it, so that no value moves the key elsewhere. */
export function parameterValueIsValid(kind: ProviderPresetParameterKind, value: string): boolean {
  if (kind === 'domain_label') {
    return (
      value.length <= LONGEST_LABEL &&
      /^[a-z0-9-]+$/u.test(value) &&
      !value.startsWith('-') &&
      !value.endsWith('-')
    );
  }
  return (
    value.length <= LONGEST_SEGMENT &&
    /^[A-Za-z0-9._~-]+$/u.test(value) &&
    !/^\.{1,2}$/u.test(value)
  );
}

/** The template with each parameter at its value; none while a value is missing or invalid. */
export function fillBaseUrl(
  template: BaseUrlTemplate,
  values: Readonly<Record<string, string>>,
): string | undefined {
  let filled = template.base_url;
  for (const parameter of templateParameters(template)) {
    const value = Object.hasOwn(values, parameter.name) ? values[parameter.name] : undefined;
    if (value === undefined || !parameterValueIsValid(parameter.kind, value)) return undefined;
    filled = filled.replaceAll(`{${parameter.name}}`, value);
  }
  return filled;
}

const standIn = (index: number): string => `aura-parameter-${index}`;

function parse(value: string): URL | undefined {
  try {
    return new URL(value);
  } catch {
    return undefined;
  }
}

const labels = (url: URL): string[] => url.hostname.replace(/\.$/u, '').split('.');
const segments = (url: URL): string[] => url.pathname.replace(/\/+$/u, '').split('/');

/** The values with which the template is `baseUrl`, compared as Core compares a connection
 *  with a route: the host's case, a default port, a trailing dot of the host and a trailing
 *  slash do not count, and an address with a query, a fragment or a user name is no route's,
 *  nor is any the route's whose template names a parameter twice or not at all. */
export function matchBaseUrl(
  template: BaseUrlTemplate,
  baseUrl: string,
): Record<string, string> | undefined {
  const parameters = templateParameters(template);
  let marked = template.base_url;
  parameters.forEach((parameter, index) => {
    marked = marked.replaceAll(`{${parameter.name}}`, standIn(index));
  });
  const expected = parse(marked);
  const actual = parse(baseUrl);
  if (
    expected === undefined ||
    actual === undefined ||
    actual.protocol !== expected.protocol ||
    actual.port !== expected.port ||
    actual.username !== '' ||
    actual.password !== '' ||
    actual.href.includes('?') ||
    actual.href.includes('#')
  ) {
    return undefined;
  }
  const values: Record<string, string> = {};
  for (const [want, got] of [
    [labels(expected), labels(actual)],
    [segments(expected), segments(actual)],
  ] as const) {
    if (want.length !== got.length) return undefined;
    for (const [index, part] of want.entries()) {
      const value = got[index]!;
      const at = parameters.findIndex((_, position) => standIn(position) === part);
      if (at < 0) {
        if (part !== value) return undefined;
        continue;
      }
      const parameter = parameters[at]!;
      if (!parameterValueIsValid(parameter.kind, value) || Object.hasOwn(values, parameter.name)) {
        return undefined;
      }
      values[parameter.name] = value;
    }
  }
  return Object.keys(values).length === parameters.length ? values : undefined;
}
