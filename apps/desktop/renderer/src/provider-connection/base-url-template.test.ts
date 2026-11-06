import type { ProviderPresetParameterView } from '@aura/contracts/generated';
import { expect, test } from 'vitest';

import { fillBaseUrl, matchBaseUrl, parameterValueIsValid } from './base-url-template';

// The same cases as Core's address_templates tests: a value stands for one whole host label or
// one whole path segment, and no address of another host, path, scheme or port is the route.

const parameter = (
  name: string,
  kind: ProviderPresetParameterView['kind'],
  example: string,
): ProviderPresetParameterView => ({ display_name: name, example, kind, name });

const GATEWAY = {
  base_url: 'https://{account}.gateway.example.test/v1/{gateway}/openai',
  base_url_parameters: [
    parameter('account', 'domain_label', 'acme'),
    parameter('gateway', 'path_segment', 'main'),
  ],
};

test('a value stands for one whole label or one whole segment', () => {
  for (const value of ['us-east-1', 'a', 'x1', '0', 'xn--fiq228c', 'a'.repeat(63)]) {
    expect(parameterValueIsValid('domain_label', value), value).toBe(true);
  }
  for (const value of [
    '',
    '-a',
    'a-',
    'a.b',
    'a/b',
    'a@b',
    'a:1',
    'US-EAST-1',
    'a b',
    'a%2e',
    'a_b',
    'évian',
    'a'.repeat(64),
  ]) {
    expect(parameterValueIsValid('domain_label', value), value).toBe(false);
  }
  for (const value of ['main', 'A_b.c~-1', '...a', 'a'.repeat(128)]) {
    expect(parameterValueIsValid('path_segment', value), value).toBe(true);
  }
  for (const value of [
    '',
    '.',
    '..',
    'a/b',
    'a?b',
    'a#b',
    'a@b',
    'a%2Fb',
    'a b',
    'a\b',
    'a'.repeat(129),
  ]) {
    expect(parameterValueIsValid('path_segment', value), value).toBe(false);
  }
});

test('a template filled with valid values is its route and gives them back', () => {
  const given = { account: 'acme', gateway: 'main' };
  expect(fillBaseUrl(GATEWAY, given)).toBe('https://acme.gateway.example.test/v1/main/openai');
  for (const address of [
    'https://acme.gateway.example.test/v1/main/openai',
    'https://ACME.Gateway.Example.Test./v1/main/openai/',
    'https://acme.gateway.example.test:443/v1/main/openai',
  ]) {
    expect(matchBaseUrl(GATEWAY, address), address).toEqual(given);
  }
  for (const missingOrInvalid of [
    { account: 'acme' },
    { account: 'evil.test', gateway: 'main' },
    { account: 'acme', gateway: 'a/b' },
    { account: '', gateway: 'main' },
  ]) {
    expect(fillBaseUrl(GATEWAY, missingOrInvalid)).toBeUndefined();
  }
});

test('no address of another host, path, scheme or port is the route', () => {
  for (const address of [
    'https://evil.test/v1/main/openai',
    'https://acme.evil.gateway.example.test/v1/main/openai',
    'https://acme.gateway.example.test.evil.test/v1/main/openai',
    'https://gateway.example.test/v1/main/openai',
    'https://a_b.gateway.example.test/v1/main/openai',
    'https://acme.gateway.example.test/v1/main/extra/openai',
    'https://acme.gateway.example.test/v1/main/openai/extra',
    'https://acme.gateway.example.test/v1/openai',
    'https://acme.gateway.example.test/v2/main/openai',
    'https://acme.gateway.example.test/v1/a%2Fb/openai',
    'https://acme.gateway.example.test/v1/main/openai?account=evil',
    'https://acme.gateway.example.test/v1/main/openai#account=evil',
    'https://user@acme.gateway.example.test/v1/main/openai',
    'http://acme.gateway.example.test/v1/main/openai',
    // The template's port under another scheme.
    'http://acme.gateway.example.test:443/v1/main/openai',
    'https://acme.gateway.example.test:8443/v1/main/openai',
    'not a url',
  ]) {
    expect(matchBaseUrl(GATEWAY, address), address).toBeUndefined();
  }
});

test("a template that names a value twice, or not at all, is no connection's route", () => {
  const twice = {
    ...GATEWAY,
    base_url: 'https://{account}.gateway.example.test/v1/{gateway}/{gateway}',
  };
  for (const address of [
    'https://acme.gateway.example.test/v1/main/main',
    'https://acme.gateway.example.test/v1/main/openai',
  ]) {
    expect(matchBaseUrl(twice, address), address).toBeUndefined();
  }
  const notAtAll = {
    ...GATEWAY,
    base_url: 'https://{account}.gateway.example.test/v1/main/openai',
  };
  expect(
    matchBaseUrl(notAtAll, 'https://acme.gateway.example.test/v1/main/openai'),
  ).toBeUndefined();
});

test('an address without parameters is its own as Core compares base URLs', () => {
  const plain = { base_url: 'https://opencode.ai/inference/go/openai/v1' };
  expect(matchBaseUrl(plain, 'https://OPENCODE.ai./inference/go/openai/v1/')).toEqual({});
  expect(matchBaseUrl(plain, 'https://opencode.ai/inference/go/anthropic/v1')).toBeUndefined();
  expect(fillBaseUrl(plain, {})).toBe('https://opencode.ai/inference/go/openai/v1');
});
