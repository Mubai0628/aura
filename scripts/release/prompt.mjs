// Passphrases are typed into the console, never echoed, and kept only in this process.
const ENTER = new Set([String.fromCharCode(13), String.fromCharCode(10)]);
const CTRL_C = String.fromCharCode(3);
const BACKSPACE = new Set([String.fromCharCode(8), String.fromCharCode(127)]);
const MINIMUM_LENGTH = 12;

/** One line typed without echo. */
export function askHidden(question, { stdin = process.stdin, stdout = process.stdout } = {}) {
  if (!stdin.isTTY) {
    return Promise.reject(new Error('a passphrase is typed in an interactive console'));
  }
  stdout.write(question);
  stdin.setRawMode(true);
  stdin.resume();
  stdin.setEncoding('utf8');
  let value = '';
  return new Promise((resolve, reject) => {
    const finish = (error) => {
      stdin.off('data', onData);
      stdin.setRawMode(false);
      stdin.pause();
      stdout.write('\n');
      if (error) reject(error);
      else resolve(value);
    };
    const onData = (chunk) => {
      for (const character of chunk) {
        if (ENTER.has(character)) return finish();
        if (character === CTRL_C) return finish(new Error('cancelled'));
        if (BACKSPACE.has(character)) value = value.slice(0, -1);
        else value += character;
      }
    };
    stdin.on('data', onData);
  });
}

/** A new passphrase, typed twice; at least 12 characters. */
export async function askNewPassphrase(label, options) {
  for (;;) {
    const first = await askHidden(`${label}（至少 ${MINIMUM_LENGTH} 个字符）：`, options);
    if (first.length < MINIMUM_LENGTH) {
      (options?.stdout ?? process.stdout).write(`太短，至少 ${MINIMUM_LENGTH} 个字符。\n`);
      continue;
    }
    const second = await askHidden(`再输入一次${label}：`, options);
    if (first === second) return first;
    (options?.stdout ?? process.stdout).write('两次不一致，请重新输入。\n');
  }
}

export { MINIMUM_LENGTH };
