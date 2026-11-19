import { useEffect, useState } from 'react';
import { PixelRing } from '../ui/PixelRing';
import './query/query.css';

const COUNTDOWN_SECONDS = 30;

function WaitCountdownChip({
  remaining,
  approval = false,
}: {
  remaining: number;
  approval?: boolean;
}) {
  const left = Math.min(1, Math.max(0, remaining / COUNTDOWN_SECONDS));
  return (
    <span className="agent-query-countdown" data-ending={remaining <= 10 || undefined} role="timer">
      <PixelRing className="agent-query-countdown-ring" share={left} />
      {remaining > 0
        ? `${remaining} 秒后视为暂时没有回应`
        : approval
          ? '等待 Core 确认超时…'
          : '正在告诉 AI 你暂时没有回应…'}
    </span>
  );
}

export function WaitCountdown({
  deadlineAt,
  focusedUntil,
  held,
  approval = false,
}: {
  deadlineAt: string | null | undefined;
  focusedUntil: string | null | undefined;
  held: boolean;
  approval?: boolean;
}) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, []);
  if (!deadlineAt || held || (focusedUntil && Date.parse(focusedUntil) > now)) return null;
  const remaining = Math.max(0, Math.ceil((Date.parse(deadlineAt) - now) / 1_000));
  return remaining <= COUNTDOWN_SECONDS ? (
    <WaitCountdownChip approval={approval} remaining={remaining} />
  ) : null;
}
