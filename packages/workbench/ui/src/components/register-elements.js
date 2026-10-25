/** Register an explicit component set without importing other UI families. */
export function register(registry, elements) {
  for (const [name, element] of elements) {
    const existing = registry.get(name);
    if (existing && existing !== element)
      throw new Error(`Incompatible component already registered: ${name}`);
    if (!existing) registry.define(name, element);
  }
}
