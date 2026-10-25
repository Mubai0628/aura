// @vitest-environment node
import { expect, test } from 'vitest';
import { setup, POLICY, S } from './page-test-support.js';
import { parsedRules } from './site-rules.js';

test('a released user page keeps its title and reloads its original address when shown', () => {
  const { pages, view } = setup(1);
  pages.setPolicy(POLICY);
  pages.navigate(S, 'saved', 'https://example.com/original');
  view(0).webContents.fire('did-navigate', {}, 'https://example.com/original');
  view(0).webContents.fire('page-title-updated', {}, 'Original title');
  pages.open(S, 'new');
  const restored = pages.show(S, 'saved', { x: 0, y: 0, width: 300, height: 200 });
  expect(restored).toMatchObject({
    url: 'https://example.com/original',
    title: 'Original title',
    closed: false,
  });
  expect(view(0).webContents.loads.at(-1)).toBe('https://example.com/original');
});

test('restoring a released model page uses its model access and reports a new refusal', () => {
  const { pages, view } = setup(1);
  pages.setPolicy(POLICY);
  pages.open(S, 'model');
  pages.setAccess(S, 'model', { kind: 'model', readOnly: true, approvedHosts: [] });
  pages.loadForModel(S, 'model', 'https://docs.rs/serde');
  view(0).webContents.fire('did-navigate', {}, 'https://docs.rs/serde');
  view(0).webContents.fire('page-title-updated', {}, 'Serde');
  pages.open(S, 'new');
  pages.setPolicy({ ...POLICY, allowed: [] });
  const refused = pages.open(S, 'model');
  expect(refused).toMatchObject({
    url: 'https://docs.rs/serde',
    title: 'Serde',
    closed: true,
    notice: { kind: 'blocked', text: '只读会话打开 docs.rs 需要批准' },
  });
  expect(pages.contents(S, 'model')).toBeUndefined();
  pages.setPolicy(POLICY);
  expect(pages.open(S, 'model')).toMatchObject({
    url: 'https://docs.rs/serde',
    title: 'Serde',
    closed: false,
  });
  const id = view(0).webContents.id;
  expect(
    pages.allowRequest({
      webContentsId: id,
      resourceType: 'mainFrame',
      url: 'https://other.example/',
    }),
  ).toBe(false);
});

test('a released workspace page retains its single workspace file boundary', () => {
  const { pages, view } = setup(1);
  pages.setPolicy(POLICY);
  const root = 'file:///C:/Work/Game/';
  pages.openFile(S, 'saved', `${root}index.html`, root);
  view(0).webContents.fire('did-navigate', {}, `${root}index.html`);
  pages.open(S, 'new');
  expect(pages.open(S, 'saved').url).toBe(`${root}index.html`);
  const id = view(0).webContents.id;
  expect(
    pages.allowRequest({ webContentsId: id, resourceType: 'image', url: `${root}image.png` }),
  ).toBe(true);
  expect(
    pages.allowRequest({
      webContentsId: id,
      resourceType: 'image',
      url: 'file:///C:/Work/Other/image.png',
    }),
  ).toBe(false);
});

test('explicitly closing a released page or its session forgets its address', () => {
  const { pages, view } = setup(1);
  pages.setPolicy(POLICY);
  pages.navigate(S, 'saved', 'https://example.com/');
  view(0).webContents.fire('did-navigate', {}, 'https://example.com/');
  pages.open(S, 'new');
  pages.close(S, 'saved');
  expect(pages.open(S, 'saved').url).toBe('about:blank');
  pages.navigate(S, 'saved', 'https://example.com/again');
  view(0).webContents.fire('did-navigate', {}, 'https://example.com/again');
  pages.open(S, 'other');
  pages.closeSession(S);
  expect(pages.open(S, 'saved').url).toBe('about:blank');
});

test('a user can navigate to a new allowed address instead of restoring a blocked old address', () => {
  const { pages, view } = setup(1);
  pages.setPolicy(POLICY);
  pages.navigate(S, 'saved', 'https://old.example/');
  view(0).webContents.fire('did-navigate', {}, 'https://old.example/');
  pages.open(S, 'new');
  pages.setPolicy({ ...POLICY, blocked: parsedRules(['old.example']) });
  expect(pages.open(S, 'saved').closed).toBe(true);
  expect(pages.navigate(S, 'saved', 'https://new.example/')).toBeUndefined();
  expect(view(0).webContents.loads).not.toContain('https://old.example/');
  expect(view(0).webContents.loads.at(-1)).toBe('https://new.example/');
});
