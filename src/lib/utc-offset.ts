/** Date.getTimezoneOffset() は「UTC - 現地時刻」の分数。 */
export function formatUtcOffset(timezoneOffsetMinutes: number): string {
  const minutes = Math.abs(timezoneOffsetMinutes);
  const sign = timezoneOffsetMinutes <= 0 ? '+' : '-';
  return `${sign}${String(Math.floor(minutes / 60)).padStart(2, '0')}:${String(minutes % 60).padStart(2, '0')}`;
}
