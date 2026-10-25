import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { BranchCopying } from './BranchCopying';

afterEach(cleanup);

// The branch opens with its newest turns; the top says the rest is coming.
test('a branch still copying says so at the top, as the thinking step loads', () => {
  render(<BranchCopying copying={{ copied_turns: 6, total_turns: 24, failed: false }} />);
  const line = screen.getByRole('status');
  expect(line.textContent).toBe('之前的会话消息正在克隆6 / 24 轮');
  expect(line.querySelector('.agent-turn-spinner')).not.toBeNull();
  expect(line.querySelector('.agent-branch-copying-label')?.textContent).toBe(
    '之前的会话消息正在克隆',
  );
});

test('a copy that stopped says what is there and what to do', () => {
  render(<BranchCopying copying={{ copied_turns: 9, total_turns: 24, failed: true }} />);
  const line = screen.getByRole('alert');
  expect(line.textContent).toContain('没能克隆完成');
  expect(line.textContent).toContain('9 / 24');
  expect(line.textContent).toContain('重新新建分支');
});
