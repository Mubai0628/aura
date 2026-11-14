import { expect, test } from 'vitest';
import { loadFailureText } from './load-failure.js';

// A page that failed to load read 「无法打开 https://…（ERR_NAME_NOT_RESOLVED）」: a code, no cause
// in words and no next step.
test('a page that did not open says why in words and what to do, with the code kept last', () => {
  expect(loadFailureText(-105, 'ERR_NAME_NOT_RESOLVED')).toBe(
    '找不到这个网站（域名解析失败），页面没有打开。请检查网址，或检查网络和代理后点刷新。（ERR_NAME_NOT_RESOLVED）',
  );
  expect(loadFailureText(-106, 'ERR_INTERNET_DISCONNECTED')).toContain('电脑当前没有联网');
  expect(loadFailureText(-130, 'ERR_PROXY_CONNECTION_FAILED')).toContain('代理连不上');
  expect(loadFailureText(-202, 'ERR_CERT_AUTHORITY_INVALID')).toContain('安全证书有问题');
  expect(loadFailureText(-102, 'ERR_CONNECTION_REFUSED')).toContain('端口');
});

test('a code without words of its own still says the page did not open and what to try', () => {
  const text = loadFailureText(-9999, 'ERR_SOMETHING_NEW');
  expect(text).toContain('页面没有打开');
  expect(text).toContain('点刷新再试');
  expect(text.endsWith('（ERR_SOMETHING_NEW）')).toBe(true);
});
