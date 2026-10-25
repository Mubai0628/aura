// @vitest-environment node
import { expect, test } from 'vitest';
import { checkedMediaResult } from './results.js';
import type { MediaCommand } from './types.js';

const command: MediaCommand = {
  command_id: 'mcm-frames',
  runtime_instance_id: 'runtime',
  session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  file_size: 100,
  media_type: 'video/webm',
  timeout_ms: 30_000,
  action: { kind: 'frames', times: [1], count: null, sheet: false, max_side: 320 },
};
const image = () => ({
  media_type: 'image/jpeg',
  data_base64: Buffer.from([255, 216, 255, 224, 255, 217]).toString('base64'),
  width: 320,
  height: 180,
});
const result = () => ({
  kind: 'frames',
  frames: [{ requested_time: 1, actual_time: 1.04, image: image() }],
  sheet: null,
});

test('checks each JPEG boundary, dimensions, requested time and exact frame fields', () => {
  expect(checkedMediaResult(result(), command)).toEqual(result());
  for (const mutation of [
    { requested_time: 2 },
    { actual_time: Infinity },
    { path: 'file:///C:/secret' },
    { image: { ...image(), data_base64: Buffer.from('not an image').toString('base64') } },
    { image: { ...image(), width: 321 } },
    { image: { ...image(), data_base64: Buffer.alloc(256 * 1024 + 1).toString('base64') } },
  ])
    expect(() =>
      checkedMediaResult(
        { ...result(), frames: [{ ...result().frames[0], ...mutation }] },
        command,
      ),
    ).toThrow();
});

test('sheet output carries just the bounded mosaic image and null frame images', () => {
  const sheetCommand: MediaCommand = {
    ...command,
    action: { kind: 'frames', times: [1], count: null, sheet: true, max_side: 320 },
  };
  const good = {
    ...result(),
    frames: [{ requested_time: 1, actual_time: 1.04, image: null }],
    sheet: image(),
  };
  expect(checkedMediaResult(good, sheetCommand)).toEqual(good);
  expect(() => checkedMediaResult({ ...good, frames: result().frames }, sheetCommand)).toThrow();
  expect(() =>
    checkedMediaResult({ ...good, sheet: { ...image(), width: 640 } }, sheetCommand),
  ).toThrow();
  expect(() => checkedMediaResult({ ...good, sheet: null }, sheetCommand)).toThrow();
});
