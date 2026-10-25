import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FileLinkContext } from './file-links';
import { readFile } from './file-reads';
import { entryName, fileSource, zipEntry } from './file-sources';
import { root, session, workspace } from './files-fixture';
import { entryNamed, listedFiles, zipEntryFinder } from './zip-entries';
import { MarkdownBody } from '../markdown-body';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';

// The model named the
// files of a zip the user attached from outside the workspace; a click on one opens that entry.

const GRANT = 'xgr_09GPXQQ03T96N1N3JCQHEYG3TB';
const LISTING = 'art_01M5P7141ZA9PE2D5CMS34V0GC';
const TEXT = [
  '共 6 项（仅列路径和未压缩大小，不含文件内容）：',
  '- 00_先读说明_补测修正版.txt — 2512 B',
  '- 01_Wokwi代码/ — 0 B [目录]',
  '- 01_Wokwi代码/STM32_Wokwi_Project/README_zh.md — 11139 B',
  '- 01_Wokwi代码/STM32_Wokwi_Project/sketch.ino — 16384 B',
  '- 01_Wokwi代码/STM32_Wokwi_Project/prebuilt/README_zh.md — 900 B',
  '- 03_报告/密钥.txt — 10 B [加密]',
].join('\n');

const notify = vi.fn();
const readText = vi.fn();
const stat = vi.fn();
const attachmentFile = vi.fn();
beforeEach(() => {
  resetToolTabsForTests();
  readText.mockReset().mockResolvedValue({ ok: true, value: { text: TEXT } });
  stat.mockReset().mockResolvedValue({ ok: false, error: { code: 'file.not_found' } });
  attachmentFile.mockReset().mockResolvedValue({
    ok: true,
    kind: 'text',
    text: 'void setup() {}',
    truncated: false,
    size: 15,
  });
  vi.stubGlobal('aura', {
    v1: {
      readAttachmentText: readText,
      statWorkspaceFile: stat,
      globWorkspacePaths: vi.fn(async () => ({ ok: true, value: { paths: [] } })),
      attachmentFile,
    },
  });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("a listing's files are its entries, not its folders or what is encrypted", () => {
  expect(listedFiles(TEXT)).toEqual([
    '00_先读说明_补测修正版.txt',
    '01_Wokwi代码/STM32_Wokwi_Project/README_zh.md',
    '01_Wokwi代码/STM32_Wokwi_Project/sketch.ino',
    '01_Wokwi代码/STM32_Wokwi_Project/prebuilt/README_zh.md',
  ]);
});

test('a link names an entry by its whole path or its last parts, nearest the root', () => {
  const files = listedFiles(TEXT);
  expect(entryNamed(files, '01_Wokwi代码/STM32_Wokwi_Project/sketch.ino')).toBe(
    '01_Wokwi代码/STM32_Wokwi_Project/sketch.ino',
  );
  expect(entryNamed(files, 'sketch.ino')).toBe('01_Wokwi代码/STM32_Wokwi_Project/sketch.ino');
  expect(entryNamed(files, 'prebuilt/README_zh.md')).toBe(
    '01_Wokwi代码/STM32_Wokwi_Project/prebuilt/README_zh.md',
  );
  // Two of that name: the one nearest the root is meant.
  expect(entryNamed(files, 'README_zh.md')).toBe('01_Wokwi代码/STM32_Wokwi_Project/README_zh.md');
  expect(entryNamed(files, 'ino')).toBeUndefined();
  expect(entryNamed(files, '没有.md')).toBeUndefined();
});

test('an entry is shown in a 文件 tab of its own, read through Main with the entry named', async () => {
  const entry = zipEntry(GRANT, '01_Wokwi代码/STM32_Wokwi_Project/sketch.ino');
  expect(fileSource(entry)).toEqual({
    kind: 'attachment',
    grant: GRANT,
    name: 'sketch.ino',
    entry: '01_Wokwi代码/STM32_Wokwi_Project/sketch.ino',
  });
  expect(entryName(entry)).toBe('sketch.ino');
  const shown = await readFile({ session, workspace }, entry);
  expect(attachmentFile).toHaveBeenCalledWith({
    action: 'text',
    session_id: session,
    external_grant_id: GRANT,
    entry: '01_Wokwi代码/STM32_Wokwi_Project/sketch.ino',
  });
  expect(shown).toMatchObject({ kind: 'text' });
});

test("a click on a zip's file in the conversation opens it; the listing is read once", async () => {
  const user = userEvent.setup();
  const zipEntryNamed = zipEntryFinder([{ grant: GRANT, listing: LISTING }], session)!;
  render(
    <FileLinkContext.Provider value={{ session, workspace, root, notify, zipEntryNamed }}>
      <MarkdownBody text={'- `sketch.ino`\n- `01_Wokwi代码/STM32_Wokwi_Project/README_zh.md`'} />
    </FileLinkContext.Provider>,
  );
  const opened = () =>
    toolTabs.get(session).tabs.map((tab) => (tab.kind === 'files' ? tab.file : tab.kind));
  await user.click(screen.getByRole('button', { name: '打开 sketch.ino' }));
  await vi.waitFor(() =>
    expect(opened()).toContain(zipEntry(GRANT, '01_Wokwi代码/STM32_Wokwi_Project/sketch.ino')),
  );
  await user.click(
    screen.getByRole('button', { name: '打开 01_Wokwi代码/STM32_Wokwi_Project/README_zh.md' }),
  );
  await vi.waitFor(() =>
    expect(opened()).toContain(zipEntry(GRANT, '01_Wokwi代码/STM32_Wokwi_Project/README_zh.md')),
  );
  expect(readText).toHaveBeenCalledTimes(1);
  expect(readText).toHaveBeenCalledWith({ session_id: session, artifact_id: LISTING });
});
