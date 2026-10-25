import type { MediaAction, MediaDecoded, MediaImage } from './types.js';

/** Compiled source runs inside the sandbox, closing over no Main values or APIs. */
export async function mediaPageRun(input: {
  url: string;
  size: number;
  action: MediaAction;
  container: string | null;
  timeout: number;
}): Promise<{ ok: true; value: MediaDecoded } | { ok: false; refused: boolean; message: string }> {
  const video = document.createElement('video');
  video.width = 320;
  video.height = 180;
  video.muted = true;
  video.preload = 'auto';
  document.body.append(video);
  let capture: MediaStream | undefined;
  const deadline = performance.now() + input.timeout;
  const remaining = () => Math.max(1, deadline - performance.now());
  const failure = (message: string): never => {
    throw new Error(message);
  };
  const event = (name: string, ready: () => boolean = () => true) =>
    new Promise<void>((resolve, reject) => {
      const finish = (error?: Error) => {
        clearTimeout(timer);
        video.removeEventListener(name, listen);
        video.removeEventListener('error', broken);
        if (error) reject(error);
        else resolve();
      };
      const listen = () => {
        if (ready()) finish();
      };
      const broken = () =>
        finish(new Error('无法解码这个视频：Chromium或系统不支持它的容器或编码。'));
      const timer = setTimeout(() => finish(new Error('媒体解码超时，已停止读取。')), remaining());
      video.addEventListener(name, listen);
      video.addEventListener('error', broken);
    });
  const seek = async (time: number) => {
    video.pause();
    if (Math.abs(video.currentTime - time) < 0.000001 && video.readyState >= 2) return;
    const moved = event('seeked');
    video.currentTime = time;
    await moved;
  };
  const frame = (maxSide: number): Promise<{ actual: number; canvas: HTMLCanvasElement }> =>
    new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error('视频没有呈现所请求的帧，已停止抽帧。')),
        remaining(),
      );
      const broken = () => {
        clearTimeout(timer);
        video.cancelVideoFrameCallback(id);
        reject(new Error('视频帧解码失败。'));
      };
      video.addEventListener('error', broken, { once: true });
      const id = video.requestVideoFrameCallback((_now, metadata) => {
        video.pause();
        clearTimeout(timer);
        video.removeEventListener('error', broken);
        if (!Number.isFinite(metadata.mediaTime) || metadata.mediaTime < 0) {
          reject(new Error('视频帧没有有效的呈现时间。'));
          return;
        }
        const canvas = document.createElement('canvas');
        const scale = Math.min(1, maxSide / Math.max(video.videoWidth, video.videoHeight));
        canvas.width = Math.max(1, Math.floor(video.videoWidth * scale));
        canvas.height = Math.max(1, Math.floor(video.videoHeight * scale));
        canvas.getContext('2d')!.drawImage(video, 0, 0, canvas.width, canvas.height);
        resolve({ actual: metadata.mediaTime, canvas });
      });
      void video.play().catch((error: unknown) => {
        clearTimeout(timer);
        video.cancelVideoFrameCallback(id);
        reject(error);
      });
    });
  const fit = (source: HTMLCanvasElement, max: number) => {
    const scale = Math.min(1, max / Math.max(source.width, source.height));
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(source.width * scale));
    canvas.height = Math.max(1, Math.round(source.height * scale));
    canvas.getContext('2d')!.drawImage(source, 0, 0, canvas.width, canvas.height);
    return canvas;
  };
  const jpeg = (original: HTMLCanvasElement, max: number): MediaImage => {
    let canvas = fit(original, max);
    for (let pass = 0; pass < 8; pass++) {
      for (const quality of [0.88, 0.72, 0.52, 0.32]) {
        const data = canvas.toDataURL('image/jpeg', quality).split(',')[1]!;
        if (atob(data).length <= 256 * 1024)
          return {
            media_type: 'image/jpeg',
            data_base64: data,
            width: canvas.width,
            height: canvas.height,
          };
      }
      canvas = fit(canvas, Math.max(1, Math.floor(Math.max(canvas.width, canvas.height) * 0.75)));
    }
    return failure('视频帧超过输出大小上限。');
  };
  try {
    const metadata = event('loadedmetadata');
    video.src = input.url;
    video.load();
    await metadata;
    if (!Number.isFinite(video.duration)) {
      // Only the decoder's finite result is used for a streaming WebM, never an assumed length.
      const finite = event(
        'durationchange',
        () => Number.isFinite(video.duration) && video.duration > 0,
      );
      video.currentTime = 1e12;
      await finite;
    }
    if (!Number.isFinite(video.duration) || video.duration <= 0)
      failure('无法确定这个视频的有限时长。');
    if (
      !video.videoWidth ||
      !video.videoHeight ||
      video.videoWidth > 32768 ||
      video.videoHeight > 32768
    )
      failure('这个文件没有可解码的视频轨，或画面尺寸过大。');
    await seek(0);
    await frame(1);
    const tracks = video as HTMLVideoElement & {
      captureStream?: () => MediaStream;
      audioTracks?: { length: number };
    };
    let hasAudio: boolean | null = null;
    if (tracks.audioTracks) hasAudio = tracks.audioTracks.length > 0;
    else if (tracks.captureStream) {
      capture = tracks.captureStream();
      if (capture.getVideoTracks().length) hasAudio = capture.getAudioTracks().length > 0;
    }
    if (input.action.kind === 'probe')
      return {
        ok: true,
        value: {
          kind: 'probe',
          duration: video.duration,
          width: video.videoWidth,
          height: video.videoHeight,
          frame_rate: null,
          has_audio: hasAudio,
          container: input.container,
          codec: null,
          byte_length: input.size,
        },
      };
    const action = input.action;
    const times =
      action.times ??
      Array.from(
        { length: action.count! },
        (_, at) => ((at + 0.5) * video.duration) / action.count!,
      );
    if (times.some((time) => time >= video.duration))
      return { ok: false, refused: true, message: '请求的取帧时间超出视频时长。' };
    // Bound retained RGBA surfaces before drawing; an 8K source must never create twelve
    // full-resolution canvases just to produce a 1024px contact sheet.
    const captureMax = action.sheet
      ? Math.max(1, Math.ceil(action.max_side / Math.ceil(Math.sqrt(times.length))))
      : action.max_side;
    const captured = [];
    for (const time of times) {
      await seek(Math.min(time, Math.max(0, video.duration - 0.001)));
      const result = await frame(captureMax);
      captured.push({ requested_time: time, actual_time: result.actual, canvas: result.canvas });
    }
    let sheet: MediaImage | null = null;
    if (action.sheet) {
      const cols = Math.ceil(Math.sqrt(captured.length));
      const rows = Math.ceil(captured.length / cols);
      // The time band uses final output pixels, rather than shrinking 28 source pixels
      // to less than one pixel for 8K video. Even the smallest sheet keeps labels in-cell.
      const label = Math.max(6, Math.min(16, Math.floor(action.max_side / (rows * 8))));
      const width = Math.max(
        1,
        Math.min(
          Math.floor(action.max_side / cols),
          Math.floor(
            ((Math.floor(action.max_side / rows) - label) * video.videoWidth) / video.videoHeight,
          ),
        ),
      );
      const imageHeight = Math.min(
        Math.floor(action.max_side / rows) - label,
        Math.max(1, Math.floor((width * video.videoHeight) / video.videoWidth)),
      );
      const height = imageHeight + label;
      const canvas = document.createElement('canvas');
      canvas.width = cols * width;
      canvas.height = rows * height;
      const ctx = canvas.getContext('2d')!;
      ctx.fillStyle = '#17191b';
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      captured.forEach((item, at) => {
        const x = (at % cols) * width;
        const y = Math.floor(at / cols) * height;
        ctx.drawImage(item.canvas, x, y, width, imageHeight);
        ctx.fillStyle = '#fff';
        ctx.font = `${Math.max(4, Math.floor(label * 0.75))}px sans-serif`;
        ctx.fillText(
          `${item.actual_time.toFixed(3)} s`,
          x + 1,
          y + height - 2,
          Math.max(1, width - 2),
        );
      });
      sheet = jpeg(canvas, action.max_side);
    }
    return {
      ok: true,
      value: {
        kind: 'frames',
        frames: captured.map(({ canvas, ...item }) => ({
          ...item,
          image: action.sheet ? null : jpeg(canvas, action.max_side),
        })),
        sheet,
      },
    };
  } catch (error) {
    const message =
      error instanceof Error && /[\u3400-\u9fff]/u.test(error.message)
        ? error.message.slice(0, 500)
        : 'Chromium未能解码这个视频，可能缺少对应的系统解码器。';
    return { ok: false, refused: false, message };
  } finally {
    video.pause();
    capture?.getTracks().forEach((track) => track.stop());
    video.removeAttribute('src');
    video.load();
    video.remove();
  }
}
