const twoDigits = (value: number): string => String(value).padStart(2, '0');

/**
 * A time as a person reads it, in local 24-hour time: the clock alone on the same day as `now`,
 * the month and day before it on another day. A value that is not a time is returned as it is.
 */
export function localTime(at: string, now = new Date()): string {
  const time = new Date(at);
  if (Number.isNaN(time.getTime())) return at;
  const clock = `${twoDigits(time.getHours())}:${twoDigits(time.getMinutes())}`;
  return time.toDateString() === now.toDateString()
    ? clock
    : `${time.getMonth() + 1}月${time.getDate()}日 ${clock}`;
}
