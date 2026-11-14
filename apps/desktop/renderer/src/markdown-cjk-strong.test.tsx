import { cleanup, render } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { MarkdownBlocks, MarkdownBody } from './markdown-body';

afterEach(cleanup);

const strong = () =>
  [...document.querySelectorAll('.markdown-body [data-streamdown="strong"]')].map(
    (element) => element.textContent,
  );
const shown = () => document.querySelector('.markdown-body')?.textContent;

// What models wrote in the live runs: a label ending in Chinese punctuation, the sentence right
// after it. CommonMark kept the asterisks as text.
test('bold that ends in Chinese punctuation right before Chinese text is bold', () => {
  render(
    <MarkdownBody
      text={'- **慢速工具：**已请求等待\n- **回显工具：**紧接着调用成功，返回 **“还在”**'}
    />,
  );
  expect(strong()).toEqual(['慢速工具：', '回显工具：', '“还在”']);
  expect(shown()).not.toContain('**');
});

test('bold around inline code and after a sentence end is bold too', () => {
  render(
    <MarkdownBody text={'**带 `?debug` 时的标题：**本机页面。**未观察到报错。**本次没有点击'} />,
  );
  expect(strong()).toEqual(['带 ?debug 时的标题：', '未观察到报错。']);
  expect(shown()).toBe('带 ?debug 时的标题：本机页面。未观察到报错。本次没有点击');
});

test('a person’s own words get the same bold', () => {
  render(<MarkdownBlocks programs text={'**注意：**别删'} />);
  expect(strong()).toEqual(['注意：']);
});

// Without Chinese text beside them, and where a space breaks the pair, the asterisks stay as
// CommonMark leaves them.
test('asterisks Markdown keeps as text elsewhere are left alone', () => {
  render(<MarkdownBody text={'pass **kwargs and **options here; a ** b ** c'} />);
  expect(strong()).toEqual([]);
  expect(shown()).toBe('pass **kwargs and **options here; a ** b ** c');
});

test('code keeps its asterisks', () => {
  render(<MarkdownBody text={'`**注意：**别删` 与 **说明：**保留'} />);
  expect(strong()).toEqual(['说明：']);
  expect(document.querySelector('[data-streamdown="inline-code"]')?.textContent).toBe(
    '**注意：**别删',
  );
});
