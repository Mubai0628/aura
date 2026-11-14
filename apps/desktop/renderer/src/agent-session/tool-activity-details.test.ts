import { expect, test } from 'vitest';
import { shownDetails } from './tool-activity-details';

// A failed row opened on "错误码：<code>" followed by the message written for the model.
test('a row shows the reason in words; the code and the model text only for debugging', () => {
  const details = [
    '找不到这个文件或文件夹（可能已被移动或删除），没有执行，什么都没改；模型会换个路径。',
    '错误码：fs.not_found',
    '给模型的说明：The requested file does not exist.',
    '相关字段：path',
  ];
  expect(shownDetails(details, false)).toEqual([details[0], details[3]]);
  expect(shownDetails(details, true)).toEqual(details);
});
