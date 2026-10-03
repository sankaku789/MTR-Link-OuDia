import type { Route } from './api';

const routeNameCollator = new Intl.Collator('ja', { numeric: true, sensitivity: 'base' });

/** Preserve ordering and object identity while filtering route names. */
export function filterRoutesForDisplay<T extends Pick<Route, 'name'>>(routes: readonly T[], query: string): T[] {
  const needle = query.trim().normalize('NFKC').toLocaleLowerCase('ja');
  return routes.filter(route => route.name.normalize('NFKC').toLocaleLowerCase('ja').includes(needle));
}

/** UI-only ordering: keep the API snapshot and its route objects intact. */
export function sortRoutesForDisplay<T extends Pick<Route, 'id' | 'name'>>(routes: readonly T[]): T[] {
  return [...routes].sort((left, right) =>
    routeNameCollator.compare(left.name, right.name)
    || (left.id < right.id ? -1 : left.id > right.id ? 1 : 0)
  );
}
