// @vitest-environment node
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

import { decideSite, parseSiteRule, ruleMatches, ruleText, siteOfUrl } from './site-rules.js';

// The same cases Rust runs (crates/aura-browser-domain/tests/site_rules.rs), so a rule the
// user writes means the same thing in Core and in the browser host.
const cases = JSON.parse(
  readFileSync(new URL('../../../../fixtures/browser/site-rules.v1.json', import.meta.url), 'utf8'),
) as {
  parse: { input: string; rule?: string; error?: string }[];
  match: { rule: string; url: string; matches: boolean }[];
  no_site: string[];
};

describe('browser site rules', () => {
  it('parse to their canonical text or name what is wrong', () => {
    for (const item of cases.parse) {
      const parsed = parseSiteRule(item.input);
      if (item.rule !== undefined) {
        expect(typeof parsed === 'string' ? parsed : ruleText(parsed), item.input).toBe(item.rule);
      } else {
        expect(parsed, item.input).toBe(item.error);
      }
    }
  });

  it('match hosts, subdomains and ports exactly as written', () => {
    for (const item of cases.match) {
      const rule = parseSiteRule(item.rule);
      if (typeof rule === 'string') throw new Error(`invalid rule ${item.rule}`);
      const site = siteOfUrl(item.url);
      if (site === undefined) throw new Error(`${item.url} has a site`);
      expect(ruleMatches(rule, site), `${item.rule} ${item.url}`).toBe(item.matches);
    }
  });

  it('give no site to pages that are not on the network', () => {
    for (const url of cases.no_site) expect(siteOfUrl(url), url).toBeUndefined();
  });

  it('keep a blocked site blocked even when a broader rule allows it', () => {
    const rule = (text: string) => {
      const parsed = parseSiteRule(text);
      if (typeof parsed === 'string') throw new Error(text);
      return parsed;
    };
    const allowed = [rule('*.example.com')];
    const blocked = [rule('ads.example.com')];
    const decide = (url: string) => decideSite(siteOfUrl(url)!, allowed, blocked);
    expect(decide('https://ads.example.com/x')).toBe('blocked');
    expect(decide('https://www.example.com/x')).toBe('allowed');
    expect(decide('https://other.net/')).toBe('unlisted');
  });
});
