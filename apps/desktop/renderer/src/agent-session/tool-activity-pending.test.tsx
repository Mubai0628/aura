import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { ToolActivity } from './ToolActivity';
import { ToolRecordReads } from './useToolCallDirectory';

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
  render(
    <ToolRecordReads.Provider value={{ unreadable: new Set(), missing: new Set(['absent']) }}>
      <ToolActivity callId="absent" entry={undefined} ended />
    </ToolRecordReads.Provider>,
  );
  expect(screen.getByText('未执行')).toBeTruthy();
  expect(screen.getByText('这个调用在开始前随这一步结束了，没有执行，什么都没改。')).toBeTruthy();
});

test('a call whose record could not be read says its state is unknown', () => {
  render(
    <ToolRecordReads.Provider value={{ unreadable: new Set(['bad']), missing: new Set() }}>
      <ToolActivity callId="bad" entry={undefined} ended />
    </ToolRecordReads.Provider>,
  );
  expect(screen.getByText('状态未知')).toBeTruthy();
  expect(screen.getByText('读不到这个调用的记录，状态未知；稍后会自动再试。')).toBeTruthy();
  expect(screen.queryByText('未执行')).toBeNull();
});

test('a finished turn waits for the first read before declaring an unrecorded call', () => {
  render(<ToolActivity callId="pending" entry={undefined} ended />);
  expect(screen.getByText('正在读取记录')).toBeTruthy();
  expect(screen.queryByText('未执行')).toBeNull();
});
