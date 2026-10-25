import { useEffect } from 'react';

/**
 * The intro on the startup screen: the name's letters roll in over the rain. It is counted
 * from the screen's first frame, which the screen records; until then, from the window's start.
 */
export const SPLASH_INTRO_MS = 1100;
/** The exit: a fade with a slight zoom; the element is removed after it. */
export const SPLASH_LEAVE_MS = 420;
/** After this the caption says the start is slow. */
export const SPLASH_SLOW_MS = 5000;
/** After this the splash steps aside, so the app's own status and recovery are reachable. */
export const SPLASH_GIVE_UP_MS = 20_000;

export type StartupPhase = 'waiting' | 'ready' | 'failed';

const clock = () => performance.now();

/**
 * Takes over the startup screen index.html shows before any script.
 *
 * While Core is starting the screen keeps looping; once Core is ready it leaves as soon as the
 * intro has played (a fast start still shows the whole intro, but no longer); when Core cannot
 * start it leaves at once, so the reason is on screen. A start slower than `SPLASH_GIVE_UP_MS`
 * hands over to the app, which shows Core's state and what can be done. Leaving never blocks
 * input: the screen stops taking clicks the moment it starts to fade.
 */
export function useStartupSplash(phase: StartupPhase, now: () => number = clock) {
  useEffect(() => {
    const splash = document.getElementById('aura-splash');
    if (!splash || splash.dataset.leaving === 'true') return;
    const timers: number[] = [];
    const leave = () => {
      if (splash.dataset.leaving === 'true') return;
      splash.dataset.leaving = 'true';
      splash.classList.add('is-leaving');
      timers.push(window.setTimeout(() => splash.remove(), SPLASH_LEAVE_MS));
    };
    const elapsed = now();
    if (phase === 'waiting') {
      const caption = document.getElementById('aura-splash-caption');
      timers.push(
        window.setTimeout(
          () => {
            if (caption) caption.textContent = '启动比平常慢一些，仍在等待 Aura Core';
          },
          Math.max(0, SPLASH_SLOW_MS - elapsed),
        ),
        window.setTimeout(leave, Math.max(0, SPLASH_GIVE_UP_MS - elapsed)),
      );
    } else if (phase === 'ready') {
      const introEnds = Number(splash.dataset.introEnds) || SPLASH_INTRO_MS;
      timers.push(window.setTimeout(leave, Math.max(0, introEnds - elapsed)));
    } else {
      leave();
    }
    return () => {
      // A removal already scheduled is kept: once the fade has begun the screen must go.
      for (const timer of timers) if (splash.dataset.leaving !== 'true') window.clearTimeout(timer);
    };
  }, [phase, now]);
}
