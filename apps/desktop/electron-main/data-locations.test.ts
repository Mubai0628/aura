// @vitest-environment node
import { expect, test } from 'vitest';
import { desktopLocations, launcherLocations, locationsFromCore } from './data-locations.js';
import { dataLocationText } from './window-recovery.js';

const fromLauncher = [
  'C:\\Programs\\Aura\\versions\\0.3.0\\Aura.exe',
  '--aura-parent-runtime-id=run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  '--aura-data-root=D:\\Aura\\data',
  '--aura-ui-profile=D:\\Aura\\ui-profile',
  '--aura-chrome-root=C:\\Users\\u\\AppData\\Local\\Aura\\namespaces\\h\\chrome',
];
const given = {
  dataRoot: 'D:\\Aura\\data',
  uiProfile: 'D:\\Aura\\ui-profile',
  chromeRoot: 'C:\\Users\\u\\AppData\\Local\\Aura\\namespaces\\h\\chrome',
};

test("the launcher's three locations are used as given", () => {
  expect(launcherLocations(fromLauncher)).toEqual(given);
});

test("without the launcher's arguments there are none", () => {
  expect(launcherLocations(['Aura.exe', '--safe-mode'])).toBeUndefined();
});

test('a partial set from the launcher is refused rather than completed', () => {
  expect(() => launcherLocations(fromLauncher.slice(0, 3))).toThrow(/data locations/);
  expect(() => launcherLocations([...fromLauncher.slice(0, 4), '--aura-chrome-root='])).toThrow(
    /data locations/,
  );
});

test("Core's answer gives the locations", () => {
  const stdout = `${JSON.stringify({
    locations: {
      data_root: 'X:\\data',
      ui_profile: 'X:\\ui-profile',
      chrome_root: 'C:\\chrome',
      settings_root: 'C:\\settings',
    },
  })}\n`;
  expect(locationsFromCore({ status: 0, stdout })).toEqual({
    dataRoot: 'X:\\data',
    uiProfile: 'X:\\ui-profile',
    chromeRoot: 'C:\\chrome',
  });
});

test("Core's refusal is reported with its reason", () => {
  const stdout = JSON.stringify({
    error: { code: 'location_missing', message: 'the data folder D:\\Aura is not there' },
  });
  expect(() => locationsFromCore({ status: 3, stdout })).toThrow(
    'the data folder D:\\Aura is not there',
  );
});

test('a Core that could not run, or answered nothing usable, is reported', () => {
  expect(() =>
    locationsFromCore({ status: null, stdout: '', error: new Error('spawn ENOENT') }),
  ).toThrow('spawn ENOENT');
  expect(() => locationsFromCore({ status: 0, stdout: 'not json' })).toThrow();
  expect(() =>
    locationsFromCore({ status: 0, stdout: JSON.stringify({ locations: { data_root: 'X' } }) }),
  ).toThrow();
});

test("the launcher's arguments are used and Core is not asked", () => {
  let asked = 0;
  expect(
    desktopLocations(fromLauncher, () => {
      asked += 1;
      return { status: 0, stdout: '' };
    }),
  ).toEqual(given);
  expect(asked).toBe(0);
});

test('without them Core is asked', () => {
  const stdout = JSON.stringify({
    locations: { data_root: 'A', ui_profile: 'B', chrome_root: 'C' },
  });
  expect(desktopLocations(['Aura.exe'], () => ({ status: 0, stdout }))).toEqual({
    dataRoot: 'A',
    uiProfile: 'B',
    chromeRoot: 'C',
  });
});

// The error box says in words where the data was expected and what to do; a missing folder names it.
test('a data folder that is not there is named, and any other failure says what to do', () => {
  const stdout = JSON.stringify({
    error: {
      code: 'location_missing',
      message: 'the data folder X:\\AuraData is not there: os error 3',
      location: 'X:\\AuraData',
    },
  });
  let thrown: unknown;
  try {
    locationsFromCore({ status: 3, stdout });
  } catch (error) {
    thrown = error;
  }
  expect(dataLocationText(thrown)).toBe(
    '数据文件夹 X:\\AuraData 不在了（可能在没接上的移动硬盘上，或已被移动、删除），Aura 没有启动；请接回该磁盘或把文件夹放回原处，再从开始菜单打开 Aura。',
  );
  for (const other of [
    new Error('the launcher passed an incomplete set of data locations'),
    new Error('spawn ENOENT'),
  ]) {
    expect(dataLocationText(other)).toBe(
      'Aura 没能确定数据放在哪里，没有启动；请从开始菜单的 Aura 快捷方式打开，仍不行请重新运行安装程序修复。',
    );
  }
});
