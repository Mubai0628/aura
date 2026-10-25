import { nearestPaletteColor, type Rgb } from './terminal-colors';

interface Parameter {
  raw: string;
  value: number;
  sub: number[];
}
const ESC = 0x1b;
const MAX_CSI_BYTES = 4096;
const MAX_CACHE_COLORS = 1024;
const encoder = new TextEncoder();
const parameter = (text: string, missing = 0) =>
  text === '' ? missing : Math.min(Number(text), 0x7fffffff);

function parameters(text: string): Parameter[] {
  // These are xterm's parameter and subparameter limits. Shortening RGB to indexed colors
  // must not make previously ignored trailing parameters suddenly execute.
  let subCount = 0;
  return text
    .split(';')
    .slice(0, 32)
    .map((raw) => {
      const [head = '', ...tail] = raw.split(':');
      const retained = tail.slice(0, Math.max(0, 32 - subCount));
      const sub = retained.map((part) => parameter(part, -1));
      subCount += sub.length;
      return { raw: [head, ...retained].join(':'), value: parameter(head), sub };
    });
}

function colorAt(params: Parameter[], start: number) {
  // Normalize xterm's semicolon/colon forms to [target, mode, color-space, r, g, b].
  // A semicolon form omits the color-space slot; a colon form reserves it.
  const values = [0, 0, -1, 0, 0, 0];
  let skipSpace = 0;
  let advance = 0;
  do {
    const item = params[start + advance]!;
    values[advance + skipSpace] = item.value;
    if (item.sub.length) {
      if (values[1] === 5) skipSpace = 1;
      for (let sub = 0; sub < item.sub.length && sub + advance + 1 + skipSpace < 6; sub++) {
        values[sub + advance + 1 + skipSpace] = item.sub[sub]!;
      }
      break;
    }
    if (
      (values[1] === 5 && advance + skipSpace >= 2) ||
      (values[1] === 2 && advance + skipSpace >= 5)
    )
      break;
    if (values[1]) skipSpace = 1;
  } while (++advance + start < params.length && advance + skipSpace < 6);
  return {
    values: values.map((value) => (value === -1 ? 0 : value)),
    count: Math.min(advance + 1, params.length - start),
  };
}

/** Streaming VT shim: only foreground/background RGB SGR becomes an indexed SGR. */
export class VtColorConverter {
  private state: 'text' | 'escape' | 'csi' | 'string' | 'discard-csi' = 'text';
  private osc = false;
  private csi: number[] = [];
  private prefix: number[] = [];
  private utf8C2 = false;
  private readonly cache = new Map<number, number>();
  private readonly stablePalette: readonly Rgb[];

  constructor(palette: readonly Rgb[]) {
    // RGB uses fixed cube/gray slots (16..255). ANSI 0..15 are theme-dependent, and xterm
    // brightens 0..7 on bold: choosing those would change an RGB color on SGR 1/22 or inverse.
    // Leave ordinary ANSI and xterm's default bold behavior unchanged.
    this.stablePalette = palette.slice(16);
  }

  private nearest(r: number, g: number, b: number) {
    // Match xterm's component masking, including its clamped oversized numeric parameters.
    r &= 0xff;
    g &= 0xff;
    b &= 0xff;
    const key = (r << 16) | (g << 8) | b;
    const cached = this.cache.get(key);
    if (cached !== undefined) return cached;
    const color = 16 + nearestPaletteColor(this.stablePalette, r, g, b);
    if (this.cache.size === MAX_CACHE_COLORS) this.cache.delete(this.cache.keys().next().value!);
    this.cache.set(key, color);
    return color;
  }

  private sgr(): Uint8Array | undefined {
    const text = String.fromCharCode(...this.csi);
    if (!/^[\d;:]*$/.test(text)) return;
    const params = parameters(text);
    const output: string[] = [];
    let changed = false;
    for (let index = 0; index < params.length;) {
      const target = params[index]!.value;
      if (target === 38 || target === 48 || target === 58) {
        const color = colorAt(params, index);
        if ((target === 38 || target === 48) && color.values[1] === 2) {
          output.push(
            `${target};5;${this.nearest(color.values[3]!, color.values[4]!, color.values[5]!)}`,
          );
          changed = true;
        } else output.push(...params.slice(index, index + color.count).map((item) => item.raw));
        index += color.count;
      } else {
        output.push(params[index]!.raw);
        index++;
      }
    }
    return changed ? encoder.encode(`\x1b[${output.join(';')}m`) : undefined;
  }

  private code(code: number, raw: number[], output: number[]) {
    if (code === ESC || code === 0x9b) {
      if (this.state === 'csi') output.push(...this.prefix, ...this.csi);
      else if (this.state === 'escape') output.push(ESC);
      this.csi = [];
      this.prefix = code === ESC ? [ESC, 0x5b] : raw;
      this.state = code === ESC ? 'escape' : 'csi';
      return;
    }
    if (code === 0x18 || code === 0x1a || code === 0x9c) {
      // CAN/SUB abort a sequence; ST terminates a string. An incomplete SGR has no effect.
      if (this.state === 'csi') output.push(...this.prefix, ...this.csi);
      else if (this.state === 'escape') output.push(ESC);
      this.state = 'text';
      this.csi = [];
      output.push(...raw);
      return;
    }
    if ([0x90, 0x98, 0x9d, 0x9e, 0x9f].includes(code)) {
      if (this.state === 'csi') output.push(...this.prefix, ...this.csi);
      else if (this.state === 'escape') output.push(ESC);
      this.state = 'string';
      this.osc = code === 0x9d;
      this.csi = [];
      output.push(...raw);
      return;
    }
    if (this.state === 'string') {
      output.push(...raw);
      if (this.osc && code === 7) this.state = 'text';
      return;
    }
    if (this.state === 'escape') {
      if (code === 0x5b) this.state = 'csi';
      else if ([0x5d, 0x50, 0x58, 0x5e, 0x5f].includes(code)) {
        output.push(ESC, ...raw);
        this.state = 'string';
        this.osc = code === 0x5d;
      } else if (code < 0x20 || code === 0x7f) output.push(...raw);
      else {
        output.push(ESC, ...raw);
        this.state = 'text';
      }
      return;
    }
    if (this.state === 'csi' || this.state === 'discard-csi') {
      if (code < 0x20 || code === 0x7f) {
        // Executable C0 controls do not end CSI. Emit them without putting them into SGR params.
        output.push(...raw);
        return;
      }
      const final = code >= 0x40 && code <= 0x7e;
      if (this.state === 'discard-csi') {
        if (final) this.state = 'text';
        return;
      }
      if (final) {
        const converted = code === 0x6d ? this.sgr() : undefined;
        output.push(...(converted ?? [...this.prefix, ...this.csi, ...raw]));
        this.csi = [];
        this.state = 'text';
      } else if (code > 0x7f) {
        output.push(...this.prefix, ...this.csi, ...raw);
        this.csi = [];
        this.state = 'text';
      } else if (this.csi.length < MAX_CSI_BYTES) this.csi.push(...raw);
      else {
        // Do not retain an unbounded unfinished control sequence, or pass an unsafe RGB SGR
        // through on overflow. Cancel it and resume after its final byte.
        output.push(ESC, 0x5b, 0x18);
        this.csi = [];
        this.state = 'discard-csi';
      }
      return;
    }
    output.push(...raw);
  }

  write(input: Uint8Array): Uint8Array {
    // Ordinary output (including large logs) can retain its original buffer. Only ESC and
    // UTF-8 C1 leads can begin controls; partial controls still take the streaming path.
    if (
      this.state === 'text' &&
      !this.utf8C2 &&
      input.indexOf(ESC) < 0 &&
      input.indexOf(0xc2) < 0
    ) {
      return input;
    }
    const output: number[] = [];
    for (const byte of input) {
      if (this.utf8C2) {
        this.utf8C2 = false;
        if (byte >= 0x80 && byte <= 0x9f) {
          this.code(byte, [0xc2, byte], output);
          continue;
        }
        this.code(0xc2, [0xc2], output);
      }
      // xterm receives UTF-8 bytes. C1 controls are UTF-8 C2 xx, never a bare continuation byte.
      if (byte === 0xc2) this.utf8C2 = true;
      else this.code(byte, [byte], output);
    }
    return Uint8Array.from(output);
  }
}
