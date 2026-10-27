import { describe, expect, it } from 'vitest';
import { originOf, refusal, sensitive, type Target } from './inspect.js';

const target = (change: Partial<Target> = {}): Target => ({
  tag: 'button',
  attributes: {},
  text: '',
  paymentForm: false,
  captcha: false,
  fileChooser: false,
  ...change,
});

describe('what only the user may do', () => {
  it('refuses typing into secret fields, anything in a CAPTCHA and opening a file chooser', () => {
    const password = target({ tag: 'input', attributes: { type: 'password' } });
    expect(refusal(password, 'type')).toMatch(/密码/u);
    expect(refusal(password, 'press')).toMatch(/密码/u);
    expect(refusal(password, 'click')).toBeNull();
    const code = target({ tag: 'input', attributes: { autocomplete: 'one-time-code' } });
    expect(refusal(code, 'type')).not.toBeNull();
    expect(refusal(target({ captcha: true }), 'click')).toMatch(/CAPTCHA/u);
    expect(refusal(target({ captcha: true }), 'hover')).toBeNull();
    expect(refusal(target({ fileChooser: true }), 'click')).toMatch(/选择文件/u);
    expect(refusal(target(), 'click')).toBeNull();
  });

  it('asks every time before buying, paying, deleting, sending, publishing or transferring', () => {
    for (const words of [
      'Buy now',
      'Place order',
      '立即购买',
      '删除',
      'Send',
      '发布',
      '转账',
      'Pay',
    ])
      expect(sensitive(target({ text: words }), 'click'), words).toBe(true);
    for (const words of ['Next', 'Search', 'Open menu', '下一页'])
      expect(sensitive(target({ text: words }), 'click'), words).toBe(false);
    expect(sensitive(target({ paymentForm: true }), 'type')).toBe(true);
    expect(sensitive(target({ text: 'Delete' }), 'hover')).toBe(false);
    expect(sensitive(target({ text: 'Delete' }), 'scroll')).toBe(false);
  });

  it('names a page by its origin as Core does', () => {
    expect(originOf('https://user:pw@Host.Example:8443/p?q')).toBe('https://host.example:8443');
    expect(originOf('http://localhost:5173/game#x')).toBe('http://localhost:5173');
    expect(originOf('about:blank')).toBe('about:blank');
  });
});
