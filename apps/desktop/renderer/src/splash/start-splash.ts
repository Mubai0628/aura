import { SPLASH_INTRO_MS, SPLASH_LEAVE_MS } from '../startup-splash';
import { createRainLake } from './rain-lake';

/** No more frames than this each second: the window is starting, and has other work. */
const FRAMES_PER_SECOND = 30;

/**
 * Starts the rain on the startup screen index.html shows, as soon as the first script
 * runs. The screen's timing stays with `useStartupSplash`: the lake keeps raining while Core
 * starts, dissolves while the screen fades, and stops once the screen is gone. A hidden window
 * draws nothing; anyone who asked for less motion sees one still picture.
 *
 * The lake's time starts with its first frame, however late the script ran (in development
 * that is once the whole page has loaded), so the name's reels turn in front of the viewer; the
 * screen records when they will have stopped (`data-intro-ends`) for `useStartupSplash`.
 */
export function startSplashLake(
  doc: Document = document,
  clock: () => number = () => performance.now(),
): void {
  const splash = doc.getElementById('aura-splash');
  const canvas = doc.getElementById('aura-splash-lake');
  const view = doc.defaultView;
  if (!splash || !(canvas instanceof HTMLCanvasElement) || !view) return;
  const lake = createRainLake(canvas);
  if (!lake) return;
  if (view.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
    lake.still();
    return;
  }
  let raf = 0;
  let last = 0;
  let begin: number | undefined;
  let leavingSince: number | undefined;
  const resize = () => lake.resize();
  const stop = () => {
    view.cancelAnimationFrame(raf);
    raf = 0;
  };
  const end = () => {
    stop();
    view.removeEventListener('resize', resize);
    doc.removeEventListener('visibilitychange', visibility);
  };
  const loop = (now: number) => {
    if (!splash.isConnected) return end();
    raf = view.requestAnimationFrame(loop);
    if (last && now - last < 1000 / FRAMES_PER_SECOND) return;
    last = now;
    const at = clock();
    if (begin === undefined) {
      begin = at;
      splash.dataset.introEnds = String(Math.round(at + SPLASH_INTRO_MS));
    }
    if (splash.classList.contains('is-leaving')) leavingSince ??= at;
    const leave =
      leavingSince === undefined ? 0 : Math.min(1, (at - leavingSince) / SPLASH_LEAVE_MS);
    lake.frame((at - begin) / 1000, leave);
  };
  const start = () => {
    if (!splash.isConnected) return end();
    if (!raf && !doc.hidden) raf = view.requestAnimationFrame(loop);
  };
  const visibility = () => (doc.hidden ? stop() : start());
  view.addEventListener('resize', resize);
  doc.addEventListener('visibilitychange', visibility);
  start();
}

startSplashLake();
