/**
 * A size the way Aura's setup and launcher windows show it, so one folder reads the same in
 * both: "< 0.1 MB", "3.5 MB", "875 MB", "8.61 GB".
 */
export function size(bytes: number): string {
  const mib = 1024 ** 2;
  const gib = 1024 ** 3;
  if (bytes >= gib) return `${(bytes / gib).toFixed(2)} GB`;
  if (bytes >= 10 * mib) return `${Math.round(bytes / mib)} MB`;
  if (bytes >= 104_858) return `${(bytes / mib).toFixed(1)} MB`;
  return '< 0.1 MB';
}
