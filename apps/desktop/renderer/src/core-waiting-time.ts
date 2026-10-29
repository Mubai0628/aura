import { useEffect, useState } from 'react';

/** Whole seconds since `since`, counted again every second while the caller is mounted. */
export function useWaitedSeconds(since: string): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1_000);
    return () => clearInterval(timer);
  }, []);
  return Math.max(0, Math.floor((now - Date.parse(since)) / 1_000));
}

/** In the turn duration's units: "8 秒", "1 分 5 秒". */
export const waitedText = (seconds: number): string =>
  seconds < 60 ? `${seconds} 秒` : `${Math.floor(seconds / 60)} 分 ${seconds % 60} 秒`;
