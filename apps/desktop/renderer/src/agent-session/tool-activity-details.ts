/** Detail lines Core writes for debugging: the error's code and the message the model read. */
const DEBUG_LINE = /^(?:错误码|给模型的说明)：/u;

/** The detail lines a row shows: the debugging lines only while 显示调试信息 is on. */
export const shownDetails = (details: readonly string[], debug: boolean): string[] =>
  debug ? [...details] : details.filter((line) => !DEBUG_LINE.test(line));
