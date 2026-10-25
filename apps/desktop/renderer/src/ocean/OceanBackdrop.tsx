import { useEffect, useRef } from 'react';
import { createOcean } from './ocean-scene';
import './ocean.css';

/** No more frames than this each second: the sharks swim slowly, and the window has other work. */
const FRAMES_PER_SECOND = 30;

/**
 * Behind the empty conversation: sharks in shaded tiles
 * circle the middle and pass under the frosted composer; a white haze keeps the title clear. It
 * moves only while the window shows it, and holds still for anyone who asked for less motion.
 */
export function OceanBackdrop() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const scene = createOcean(canvas);
    if (!scene) return;
    const still = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
    let raf = 0;
    let last = 0;
    const loop = (now: number) => {
      raf = window.requestAnimationFrame(loop);
      if (last && now - last < 1000 / FRAMES_PER_SECOND) return;
      const dt = last ? Math.min(0.1, (now - last) / 1000) : 0;
      last = now;
      scene.frame(dt);
    };
    const start = () => {
      if (raf || still || document.hidden) return;
      last = 0;
      raf = window.requestAnimationFrame(loop);
    };
    const stop = () => {
      window.cancelAnimationFrame(raf);
      raf = 0;
    };
    const visibility = () => (document.hidden ? stop() : start());
    const resized = new ResizeObserver(() => {
      scene.resize();
      scene.frame(0);
    });
    const where = (event: PointerEvent | MouseEvent) => {
      const box = canvas.getBoundingClientRect();
      return { x: event.clientX - box.left, y: event.clientY - box.top };
    };
    const moved = (event: PointerEvent) => scene.pointer(where(event));
    const left = () => scene.pointer(undefined);
    const poked = (event: MouseEvent) => scene.poke(where(event));
    resized.observe(canvas);
    document.addEventListener('visibilitychange', visibility);
    canvas.addEventListener('pointermove', moved);
    canvas.addEventListener('pointerleave', left);
    canvas.addEventListener('click', poked);
    scene.frame(0);
    start();
    return () => {
      stop();
      resized.disconnect();
      document.removeEventListener('visibilitychange', visibility);
      canvas.removeEventListener('pointermove', moved);
      canvas.removeEventListener('pointerleave', left);
      canvas.removeEventListener('click', poked);
    };
  }, []);

  return (
    <div aria-hidden="true" className="ocean-backdrop">
      <canvas className="ocean-canvas" ref={canvasRef} />
      <div className="ocean-haze" />
    </div>
  );
}
