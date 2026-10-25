import { exact, integer, record } from './protocol.js';
import { strictBase64 } from './source.js';
import {
  IMAGE_BYTES,
  RESULT_BYTES,
  TOTAL_IMAGE_BYTES,
  type MediaCommand,
  type MediaDecoded,
} from './types.js';

const finite = (value: unknown, min = 0): value is number =>
  typeof value === 'number' && Number.isFinite(value) && value >= min;
const text = (value: unknown) =>
  value === null || (typeof value === 'string' && value.length <= 128);

/** The sandbox's output is untrusted too: bound it before it enters the Core connection. */
export function checkedMediaResult(value: unknown, command: MediaCommand): MediaDecoded {
  function fail(): never {
    throw new Error('媒体解码返回了无效或超限的结果。');
  }
  if (!record(value) || Buffer.byteLength(JSON.stringify(value)) > RESULT_BYTES - 2048) fail();
  if (command.action.kind === 'probe') {
    if (
      !exact(
        value,
        'kind,duration,width,height,frame_rate,has_audio,container,codec,byte_length',
      ) ||
      value.kind !== 'probe' ||
      !finite(value.duration, Number.MIN_VALUE) ||
      !integer(value.width, 1, 32768) ||
      !integer(value.height, 1, 32768) ||
      !(
        value.frame_rate === null ||
        (finite(value.frame_rate, Number.MIN_VALUE) && value.frame_rate <= 1000)
      ) ||
      !(value.has_audio === null || typeof value.has_audio === 'boolean') ||
      !text(value.container) ||
      !text(value.codec) ||
      value.byte_length !== command.file_size
    )
      fail();
  } else {
    const action = command.action;
    const expected = action.times?.length ?? action.count;
    if (
      !exact(value, 'kind,frames,sheet') ||
      value.kind !== 'frames' ||
      !Array.isArray(value.frames) ||
      value.frames.length !== expected
    )
      fail();
    let total = 0;
    const image = (candidate: unknown) => {
      if (
        !record(candidate) ||
        !exact(candidate, 'media_type,data_base64,width,height') ||
        candidate.media_type !== 'image/jpeg' ||
        !integer(candidate.width, 1, action.max_side) ||
        !integer(candidate.height, 1, action.max_side)
      )
        fail();
      const bytes = strictBase64(candidate.data_base64, IMAGE_BYTES);
      if (
        bytes.length < 4 ||
        bytes[0] !== 0xff ||
        bytes[1] !== 0xd8 ||
        bytes[2] !== 0xff ||
        bytes.at(-2) !== 0xff ||
        bytes.at(-1) !== 0xd9
      )
        fail();
      total += bytes.length;
      if (total > TOTAL_IMAGE_BYTES) fail();
    };
    value.frames.forEach((frame: unknown, at: number) => {
      if (
        !record(frame) ||
        !exact(frame, 'requested_time,actual_time,image') ||
        !finite(frame.requested_time) ||
        !finite(frame.actual_time) ||
        (action.times !== null && frame.requested_time !== action.times[at])
      )
        fail();
      if (action.sheet) {
        if (frame.image !== null) fail();
      } else image(frame.image);
    });
    if (action.sheet) image(value.sheet);
    else if (value.sheet !== null) fail();
  }
  return value as unknown as MediaDecoded;
}
