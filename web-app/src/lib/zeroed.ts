/** Build a zeroed map from a static key list. */
export function zeroed<K extends string>(keys: readonly K[]): Record<K, number> {
  const out = {} as Record<K, number>;
  for (const key of keys) out[key] = 0;
  return out;
}

/** Build a map from a static key list using a factory per key. */
export function filled<K extends string, V>(keys: readonly K[], make: () => V): Record<K, V> {
  const out = {} as Record<K, V>;
  for (const key of keys) out[key] = make();
  return out;
}
