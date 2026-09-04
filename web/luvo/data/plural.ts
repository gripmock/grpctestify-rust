/** The noun for a count. One rule, so no row has to remember it.
 *
 *  English plurals are regular often enough that every place doing this by
 *  hand wrote `n === 1 ? '' : 's'` again — and the places that forgot printed
 *  `1 asserts`, `1 methods`, `1 rows`. */
export function plural(n: number, one: string, many = `${one}s`): string {
  return n === 1 ? one : many;
}

/** The count and its noun: `1 assert`, `3 asserts`. */
export function count(n: number, one: string, many?: string): string {
  return `${n} ${plural(n, one, many)}`;
}
