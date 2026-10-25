/** How long ago `at` was, as people say it: 刚刚, 5 分钟前, 2 小时前, 3 天前. */
export function ago(at: string | null | undefined, now = Date.now()): string | undefined {
  const time = at ? Date.parse(at) : Number.NaN;
  if (!Number.isFinite(time)) return undefined;
  const minutes = Math.floor(Math.max(0, now - time) / 60_000);
  if (minutes < 1) return '刚刚';
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  return `${Math.floor(hours / 24)} 天前`;
}
