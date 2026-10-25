import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { ToolActivity } from './ToolActivity';

// A call whose record is
// not read yet is a row with its state, never a loading line.
afterEach(cleanup);

test('a call not recorded yet is being prepared while its turn runs', () => {
  render(<ToolActivity entry={undefined} ended={false} />);
  expect(screen.getByText('工具调用')).toBeTruthy();
  expect(screen.getByText('准备中')).toBeTruthy();
  expect(screen.queryByText(/正在读取工具活动/u)).toBeNull();
});

test('a call that never ran says so once its turn has ended', () => {
  render(<ToolActivity entry={undefined} ended />);
  expect(screen.getByText('未执行')).toBeTruthy();
});
