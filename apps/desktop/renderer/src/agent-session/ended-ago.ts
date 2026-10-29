/**
 * How long ago a turn ended, in its two largest units and without seconds: "2 天 13 小时之前",
 * "1 小时 5 分钟之前", "53 分钟之前"; a second unit of 0 is left out ("3 小时之前"), and under a
 * minute is "刚刚". Number and unit are spaced as the turn's time is ("15 分 41 秒").
 */
export function endedAgo(endedAt: number, now: number): string {
  const minutes = Math.floor(Math.max(0, now - endedAt) / 60_000);
  if (minutes < 1) return '刚刚';
  const days = Math.floor(minutes / 1_440);
  const hours = Math.floor((minutes % 1_440) / 60);
  const units: [number, string][] =
    days > 0
      ? [
          [days, '天'],
          [hours, '小时'],
        ]
      : hours > 0
        ? [
            [hours, '小时'],
            [minutes % 60, '分钟'],
          ]
        : [[minutes, '分钟']];
  const shown = units.filter(([count], index) => index === 0 || count > 0);
  return `${shown.map(([count, unit]) => `${count} ${unit}`).join(' ')}之前`;
}

const twoDigits = (value: number) => String(value).padStart(2, '0');

/** When the turn ended, for its bubble: "2026年10月28日 21:35", local and 24-hour. */
export function endedAtText(endedAt: number): string {
  const at = new Date(endedAt);
  return `${at.getFullYear()}年${at.getMonth() + 1}月${at.getDate()}日 ${twoDigits(at.getHours())}:${twoDigits(at.getMinutes())}`;
}
