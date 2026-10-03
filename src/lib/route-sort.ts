import type { Route } from './api';

const routeNameCollator = new Intl.Collator('ja', { numeric: true, sensitivity: 'base' });

/** UI-only ordering: keep the API snapshot and its route objects intact. */
export function sortRoutesForDisplay<T extends Pick<Route, 'id' | 'name'>>(routes: readonly T[]): T[] {
  return [...routes].sort((left, right) =>
    routeNameCollator.compare(left.name, right.name)
    || (left.id < right.id ? -1 : left.id > right.id ? 1 : 0)
  );
}
