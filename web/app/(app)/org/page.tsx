"use client";

import { useMemo, useState } from "react";
import Link from "next/link";
import clsx from "clsx";
import { useMe } from "@/lib/me";
import { useQuery } from "@/lib/hooks";
import { levelName } from "@/lib/format";
import type { OrgNode } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader } from "@/components/ui/Card";
import { SearchInput } from "@/components/ui/Filters";
import { Button } from "@/components/ui/Button";
import { CardSkeleton } from "@/components/ui/Skeleton";
import { ErrorState, EmptyState } from "@/components/ui/States";
import { Badge } from "@/components/ui/Badge";

const DEFAULT_OPEN_LEVEL = 3;

function matches(node: OrgNode, q: string): boolean {
  const hay = `${node.name} ${node.title} ${node.department}`.toLowerCase();
  return hay.includes(q);
}

/** Keep nodes that match or have a matching descendant. */
function filterTree(node: OrgNode, q: string): OrgNode | null {
  const children = node.children
    .map((c) => filterTree(c, q))
    .filter((c): c is OrgNode => c !== null);
  if (matches(node, q) || children.length > 0) return { ...node, children };
  return null;
}

function collectIds(node: OrgNode, out: Set<string>, predicate: (n: OrgNode) => boolean) {
  if (predicate(node)) out.add(node.id);
  node.children.forEach((c) => collectIds(c, out, predicate));
}

function countNodes(node: OrgNode): number {
  return 1 + node.children.reduce((acc, c) => acc + countNodes(c), 0);
}

export default function OrgPage() {
  const { chain, employee } = useMe();
  const tree = useQuery<OrgNode>("org/tree");
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<Set<string> | null>(null);

  const q = query.trim().toLowerCase();
  const root = tree.data;
  const visible = useMemo(() => (root && q ? filterTree(root, q) : root), [root, q]);

  const expanded = useMemo(() => {
    if (!visible) return new Set<string>();
    if (q) {
      const all = new Set<string>();
      collectIds(visible, all, () => true);
      return all;
    }
    if (open) return open;
    const initial = new Set<string>();
    collectIds(visible, initial, (n) => n.level < DEFAULT_OPEN_LEVEL);
    return initial;
  }, [visible, q, open]);

  function toggle(id: string) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setOpen(next);
  }

  function expandAll(value: boolean) {
    if (!root) return;
    const next = new Set<string>();
    if (value) collectIds(root, next, () => true);
    setOpen(next);
  }

  const sortedChain = [...chain].sort((a, b) => a.level - b.level);

  return (
    <div>
      <PageHeader
        title="Org chart"
        description={
          root
            ? `${countNodes(root)} people in the reporting tree`
            : "Reporting lines across the company"
        }
        actions={
          <>
            <Button variant="secondary" size="sm" onClick={() => expandAll(true)}>
              Expand all
            </Button>
            <Button variant="secondary" size="sm" onClick={() => expandAll(false)}>
              Collapse all
            </Button>
          </>
        }
      />
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <div className="lg:col-span-2">
          <div className="mb-3">
            <SearchInput
              value={query}
              onChange={setQuery}
              placeholder="Search name, title or department"
            />
          </div>
          {tree.loading && !root ? (
            <CardSkeleton lines={8} />
          ) : tree.error ? (
            <ErrorState error={tree.error} onRetry={tree.reload} />
          ) : !visible ? (
            <Card>
              <CardBody>
                <EmptyState title="No one matches that search" />
              </CardBody>
            </Card>
          ) : (
            <Card>
              <CardBody className="overflow-x-auto">
                <ul role="tree" className="min-w-[20rem]">
                  <TreeNode
                    node={visible}
                    expanded={expanded}
                    onToggle={toggle}
                    meId={employee?.id ?? null}
                    depth={0}
                  />
                </ul>
              </CardBody>
            </Card>
          )}
        </div>
        <Card className="self-start">
          <CardHeader title="My chain of command" description="From the CEO down to you" />
          <CardBody>
            {sortedChain.length === 0 ? (
              <p className="text-sm text-slate-500">No chain available.</p>
            ) : (
              <ol className="space-y-2">
                {sortedChain.map((link, i) => (
                  <li
                    key={link.id}
                    className="flex items-start gap-2 text-sm"
                    style={{ paddingLeft: `${i * 0.75}rem` }}
                  >
                    <span className="mt-0.5 text-xs text-slate-400" aria-hidden="true">
                      {i === 0 ? "" : "└"}
                    </span>
                    <Link href={`/people/${link.id}`} className="min-w-0 hover:text-accent-700">
                      <span className="block truncate font-medium text-slate-900">
                        {link.name}
                        {employee && link.id === employee.id ? " (you)" : ""}
                      </span>
                      <span className="block truncate text-xs text-slate-500">
                        {link.title}, {levelName(link.level)}
                      </span>
                    </Link>
                  </li>
                ))}
              </ol>
            )}
          </CardBody>
        </Card>
      </div>
    </div>
  );
}

function TreeNode({
  node,
  expanded,
  onToggle,
  meId,
  depth,
}: {
  node: OrgNode;
  expanded: Set<string>;
  onToggle: (id: string) => void;
  meId: string | null;
  depth: number;
}) {
  const hasChildren = node.children.length > 0;
  const isOpen = expanded.has(node.id);
  return (
    <li
      role="treeitem"
      aria-expanded={hasChildren ? isOpen : undefined}
      // the row for the signed-in person is highlighted, so say so out loud
      aria-selected={node.id === meId}
    >
      <div
        className={clsx(
          "flex items-center gap-2 rounded-md py-1.5 pr-2 hover:bg-slate-50",
          node.id === meId && "bg-accent-50",
        )}
        style={{ paddingLeft: `${depth * 1.25}rem` }}
      >
        {hasChildren ? (
          <button
            type="button"
            onClick={() => onToggle(node.id)}
            className="flex h-6 w-6 shrink-0 items-center justify-center rounded text-slate-500 hover:bg-slate-200"
            aria-label={isOpen ? `Collapse ${node.name}` : `Expand ${node.name}`}
          >
            <span className="text-xs">{isOpen ? "−" : "+"}</span>
          </button>
        ) : (
          <span className="h-6 w-6 shrink-0" aria-hidden="true" />
        )}
        <Link href={`/people/${node.id}`} className="min-w-0 flex-1 text-sm hover:text-accent-700">
          <span className="font-medium text-slate-900">{node.name}</span>
          <span className="ml-2 text-slate-500">{node.title}</span>
        </Link>
        <Badge tone="neutral" className="hidden sm:inline-flex">
          {node.department}
        </Badge>
        {hasChildren ? (
          <span className="text-xs text-slate-400" title="Direct reports">
            {node.children.length}
          </span>
        ) : null}
      </div>
      {hasChildren && isOpen ? (
        <ul role="group">
          {node.children.map((child) => (
            <TreeNode
              key={child.id}
              node={child}
              expanded={expanded}
              onToggle={onToggle}
              meId={meId}
              depth={depth + 1}
            />
          ))}
        </ul>
      ) : null}
    </li>
  );
}
