import { formatMoney } from "./format";
import type { Dashboard } from "./types";

export interface StatCard {
  key: string;
  label: string;
  value: string;
  href: string;
  hint?: string;
}

/**
 * Flatten the summary into stat cards.
 *
 * The personal blocks are always shown. The wider blocks (shipments, receivables,
 * people, service desk) are present only when the caller has the permission behind
 * them, so a missing block simply contributes no cards.
 */
export function statCards(d: Dashboard): StatCard[] {
  const cards: StatCard[] = [];
  const push = (key: string, label: string, value: number | string, href: string, hint?: string) =>
    cards.push({ key, label, value: String(value), href, hint });

  if (d.my_work.open > 0 || d.my_work.overdue > 0) {
    push(
      "my_work",
      "My open tasks",
      d.my_work.open,
      "/ops/work-orders",
      d.my_work.overdue > 0 ? `${d.my_work.overdue} overdue` : "Work orders assigned to you",
    );
  }
  if (d.waiting_on_me) {
    const { leave_requests: leave, expense_claims: expenses } = d.waiting_on_me;
    if (leave + expenses > 0) {
      push(
        "waiting_on_me",
        "Waiting on you",
        leave + expenses,
        "/hr/leave?tab=approvals",
        `${leave} leave, ${expenses} expenses`,
      );
    }
  }
  if (d.my_messages.unread > 0) {
    push(
      "my_messages",
      "Unread messages",
      d.my_messages.unread,
      "/inbox",
      `Across ${d.my_messages.threads} ${d.my_messages.threads === 1 ? "thread" : "threads"}`,
    );
  }
  if (d.my_leave.pending > 0 || d.my_leave.upcoming_approved > 0) {
    push(
      "my_leave",
      "My leave",
      d.my_leave.pending,
      "/hr/leave",
      `${d.my_leave.upcoming_approved} approved and upcoming`,
    );
  }
  if (d.my_tickets.assigned_to_me > 0 || d.my_tickets.open > 0) {
    push(
      "my_tickets",
      "My tickets",
      d.my_tickets.assigned_to_me || d.my_tickets.open,
      "/support",
      d.my_tickets.awaiting_my_close > 0
        ? `${d.my_tickets.awaiting_my_close} awaiting your close`
        : "Support requests you raised",
    );
  }
  if (d.shipments) {
    push(
      "shipments",
      "Shipments in flight",
      d.shipments.total,
      "/ops/shipments",
      "Booked through out for delivery",
    );
  }
  if (d.receivables) {
    push(
      "receivable",
      "AR outstanding",
      formatMoney(d.receivables.outstanding),
      "/finance/reports?tab=aging",
      `${formatMoney(d.receivables.overdue)} overdue across ${d.receivables.overdue_invoices} invoices`,
    );
  }
  if (d.people) {
    push(
      "people",
      "Headcount",
      d.people.headcount,
      "/people",
      `${d.people.on_leave_today} on leave today, ${d.people.joined_this_month} joined this month`,
    );
  }
  if (d.service_desk) {
    push(
      "service_desk",
      "Service desk",
      d.service_desk.open,
      "/support?tab=all",
      `${d.service_desk.unassigned} unassigned, ${d.service_desk.breaching_sla} breaching SLA`,
    );
  }
  return cards;
}
