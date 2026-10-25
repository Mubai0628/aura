export function page(items: number[], cursor: number, limit: number): number[] {
  return items.slice(0, limit);
}
