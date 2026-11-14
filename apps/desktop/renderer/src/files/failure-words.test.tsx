import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import {
  attachmentUnreadable,
  fileFailure,
  indexFailureCause,
  listingFailure,
  NOT_LOCATED_WHILE_INDEXING,
} from './failure-words';
import { MediaView } from './MediaView';

afterEach(cleanup);

// A file, a folder or an index that could not be read says why in words, what it means and what
// to do with the buttons the panel really has.

const failure = (code: string, message = 'x') => ({ code, message });

test('a file that could not be read names why: locked, busy, archived or out of reach', () => {
  expect(fileFailure(failure('fs.io_failed'))).toBe(
    '读不出这个文件：它可能正被其他程序占用，或没有读取权限。关闭占用它的程序后点“重新读取”。',
  );
  expect(fileFailure(failure('fs.resource_busy'))).toBe(
    '这个文件正被另一项操作使用，请稍后点“重新读取”。',
  );
  expect(fileFailure(failure('workspace.archived'))).toBe(
    '这个工作区已归档，文件暂不能读取。请先在 设置 → 归档 里恢复。',
  );
  expect(fileFailure(failure('workspace.offline'))).toBe(
    '工作区文件夹现在访问不到（可能被移动、改名或所在磁盘未连接）。连上后点“重新读取”。',
  );
  expect(fileFailure(failure('ipc.request_timeout', 'Core request timed out'))).toBe(
    '文件没有读出来（Aura Core 没有及时回复）；请点“重新读取”。',
  );
});

test('a folder that could not be listed says why and what to do', () => {
  expect(listingFailure(failure('workspace.offline'))).toBe(
    '文件列表读不出来：工作区文件夹现在访问不到（可能被移动、改名或所在磁盘未连接）。连上后点“重试”，或在侧栏菜单点“更改位置…”。',
  );
  expect(listingFailure(failure('fs.io_failed'))).toBe(
    '文件列表读不出来：没有权限读取这个文件夹，或它正被占用。处理后点“重试”。',
  );
  expect(NOT_LOCATED_WHILE_INDEXING).toBe('还没定位到：工作区还在建立文件索引。稍后点“重试”。');
});

test("an index that failed is said in words, not the system's English", () => {
  expect(indexFailureCause('failed to read_dir D:/w: Access is denied. (os error 5)')).toBe(
    '读取工作区文件夹时被拒绝（没有权限，或磁盘不可用）',
  );
  expect(indexFailureCause('The system cannot find the path specified. (os error 3)')).toBe(
    '工作区文件夹找不到了（可能被移动、改名或所在磁盘未连接）',
  );
  expect(indexFailureCause('工作区文件夹被换掉了')).toBe('工作区文件夹被换掉了');
  expect(indexFailureCause('something else')).toBe('读取工作区文件夹时出错');
});

test('an attachment an older message carried that cannot be read leaves the message as it was', () => {
  expect(attachmentUnreadable(failure('artifact.not_found'))).toBe(
    '这个附件读不出来（记录里可能已没有它）。消息本身不受影响；需要时重新附加原文件。',
  );
  expect(attachmentUnreadable(failure('ipc.transport_lost', 'Transport is closed'))).toBe(
    '这个附件读不出来（与 Aura Core 的连接断开了）。消息本身不受影响；需要时重新附加原文件。',
  );
});

test('a picture the viewer cannot show says so, and a video says it cannot play', () => {
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: {} } });
  const scope = { session: 'ses_1', workspace: 'wsp_1' } as never;
  const { container, unmount } = render(
    <MediaView scope={scope} path="a.heic" name="a.heic" media="image" src="aura://x" actual />,
  );
  fireEvent.error(container.querySelector('img')!);
  expect(screen.getByRole('alert').textContent).toContain(
    '显示不了这张图片：内置查看器不支持它的格式，或文件已被移动或改动。可以点“用系统程序打开”。',
  );
  unmount();
  const video = render(
    <MediaView scope={scope} path="a.mkv" name="a.mkv" media="video" src="aura://y" actual />,
  );
  fireEvent.error(video.container.querySelector('video')!);
  expect(screen.getByRole('alert').textContent).toContain(
    '播放不了这个文件：内置播放器不支持它的编码，或文件已被移动或改动。可以点“用系统程序打开”。',
  );
  vi.restoreAllMocks();
});
