import type { CanonicalState } from './generated';

function entityId(value: unknown): string | undefined {
  if (value !== null && typeof value === 'object' && 'id' in value && typeof value.id === 'string')
    return value.id;
  return undefined;
}

// Canonical snapshots contain JSON values. Reuse equal branches without mutating either snapshot.
function retainEqual(previous: unknown, next: unknown): unknown {
  if (Object.is(previous, next)) return previous;
  if (
    previous === null ||
    next === null ||
    typeof previous !== 'object' ||
    typeof next !== 'object'
  )
    return next;
  if (Array.isArray(next)) {
    if (!Array.isArray(previous)) return next;
    let byId: Map<string, unknown> | undefined;
    let result: unknown[] | undefined;
    let equal = previous.length === next.length;
    for (let index = 0; index < next.length; index += 1) {
      const id = entityId(next[index]);
      let old = previous[index];
      if (id !== undefined && id !== entityId(old)) {
        byId ??= new Map(
          previous.flatMap((item) => {
            const key = entityId(item);
            return key === undefined ? [] : [[key, item] as const];
          }),
        );
        old = byId.get(id);
      }
      const retained = retainEqual(old, next[index]);
      equal &&= Object.is(retained, previous[index]);
      if (!Object.is(retained, next[index])) {
        result ??= next.slice();
        result[index] = retained;
      }
    }
    return equal ? previous : (result ?? next);
  }
  if (Array.isArray(previous)) return next;
  const old = previous as Record<string, unknown>;
  const incoming = next as Record<string, unknown>;
  const keys = Object.keys(incoming);
  let equal = Object.keys(old).length === keys.length;
  let result: Record<string, unknown> | undefined;
  for (const key of keys) {
    const retained = retainEqual(old[key], incoming[key]);
    equal &&= Object.hasOwn(old, key) && Object.is(retained, old[key]);
    if (!Object.is(retained, incoming[key])) {
      result ??= { ...incoming };
      result[key] = retained;
    }
  }
  return equal ? previous : (result ?? next);
}

/** Keeps unchanged references within one Project and session, including entities reordered by ID. */
export function reconcileCanonical(
  previous: CanonicalState | undefined,
  next: CanonicalState,
): CanonicalState {
  if (
    !previous ||
    previous.projectId !== next.projectId ||
    previous.session.sessionId !== next.session.sessionId
  )
    return next;
  return retainEqual(previous, next) as CanonicalState;
}
