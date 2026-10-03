export function formatDwell(millis: number | undefined): string {
  if (millis === undefined) return 'なし';
  return `${millis} ms（${Math.floor(millis / 60000)}分${millis % 60000 / 1000}秒）`;
}
