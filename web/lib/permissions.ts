/**
 * Permission helpers. Keys follow `resource:action[:scope]` (docs/DOMAIN.md).
 *
 * For the employee-scoped families the scopes form a strict order
 * (`all` > `department` > `subtree` > `self`), so a principal holding
 * `employees:read:all` satisfies a check for `employees:read:subtree`. A few
 * families use their own suffixes (`messages:send:chain|department|subtree|any`,
 * `messages:broadcast:subtree|company`, `expenses:approve:subtree|finance`); those
 * are compared with the small tables below and otherwise matched exactly.
 */

export type Scope = "self" | "subtree" | "department" | "all";

const STANDARD_RANK: Record<string, number> = { self: 1, subtree: 2, department: 3, all: 4 };

/** Families whose suffixes are not the standard scope ladder. */
const FAMILY_RANK: Record<string, Record<string, number>> = {
  "messages:send": { chain: 1, department: 2, subtree: 2, any: 4 },
  "messages:broadcast": { subtree: 1, company: 2 },
};

interface ParsedKey {
  family: string;
  scope: string | null;
}

export function parseKey(key: string): ParsedKey {
  const parts = key.split(":");
  if (parts.length >= 3) {
    return { family: `${parts[0]}:${parts[1]}`, scope: parts.slice(2).join(":") };
  }
  return { family: key, scope: null };
}

/** Exact key match. */
export function has(permissions: readonly string[], key: string): boolean {
  return permissions.includes(key);
}

/** True when the principal holds the family with any scope suffix (or the bare key). */
export function canAny(permissions: readonly string[], family: string): boolean {
  const prefix = `${family}:`;
  return permissions.some((p) => p === family || p.startsWith(prefix));
}

/**
 * True when the principal holds `key` exactly, or the same family at a wider scope.
 * Keys without a scope suffix must match exactly.
 */
export function can(permissions: readonly string[], key: string): boolean {
  if (has(permissions, key)) return true;
  const { family, scope } = parseKey(key);
  if (!scope) return false;
  const ranks = FAMILY_RANK[family] ?? STANDARD_RANK;
  const need = ranks[scope];
  if (need === undefined) return false;
  return permissions.some((p) => {
    const held = parseKey(p);
    if (held.family !== family || !held.scope) return false;
    if (held.scope === scope) return true;
    const rank = ranks[held.scope];
    return rank !== undefined && rank > need;
  });
}

/** True when every key passes `can`. */
export function canAll(permissions: readonly string[], keys: readonly string[]): boolean {
  return keys.every((k) => can(permissions, k));
}

/** True when at least one key passes `can`. */
export function canAnyOf(permissions: readonly string[], keys: readonly string[]): boolean {
  return keys.some((k) => can(permissions, k));
}

/**
 * The widest standard scope the principal holds for a family, or null when the
 * family is not held at all. Only the standard ladder is considered.
 */
export function scopeOf(permissions: readonly string[], family: string): Scope | null {
  let best: Scope | null = null;
  let bestRank = 0;
  for (const p of permissions) {
    const held = parseKey(p);
    if (held.family !== family || !held.scope) continue;
    const rank = STANDARD_RANK[held.scope];
    if (rank !== undefined && rank > bestRank) {
      bestRank = rank;
      best = held.scope as Scope;
    }
  }
  return best;
}

/** True when the principal can see employees beyond their own record. */
export function canSeeOthers(permissions: readonly string[]): boolean {
  const scope = scopeOf(permissions, "employees:read");
  return scope !== null && scope !== "self";
}

/** Approvers for leave and expenses: any subtree approval right or the HR override. */
export function isLeaveApprover(permissions: readonly string[]): boolean {
  return can(permissions, "leave:approve:subtree") || has(permissions, "leave:manage:all");
}

export function isExpenseApprover(permissions: readonly string[]): boolean {
  return canAny(permissions, "expenses:approve");
}

export function isSupportAgent(permissions: readonly string[]): boolean {
  return has(permissions, "tickets:manage");
}

export function canBroadcast(permissions: readonly string[]): boolean {
  return canAny(permissions, "messages:broadcast");
}
