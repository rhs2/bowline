"use client";

import { Badge } from "./ui/Badge";
import { formatCountdown } from "@/lib/format";
import { useNow } from "@/lib/hooks";
import type { Ticket } from "@/lib/types";

/** Time left to first response, or the outcome once responded or closed. */
export function SlaCountdown({ ticket }: { ticket: Pick<Ticket, "sla_due_at" | "first_response_at" | "status"> }) {
  const now = useNow(30000);
  if (ticket.first_response_at) {
    const met = new Date(ticket.first_response_at).getTime() <= new Date(ticket.sla_due_at).getTime();
    return <Badge tone={met ? "success" : "danger"}>{met ? "Responded in SLA" : "SLA missed"}</Badge>;
  }
  if (ticket.status === "resolved" || ticket.status === "closed") {
    return <Badge tone="neutral">Closed</Badge>;
  }
  const { text, overdue } = formatCountdown(ticket.sla_due_at, now);
  return <Badge tone={overdue ? "danger" : "warning"}>{overdue ? `Breached, ${text}` : `${text} left`}</Badge>;
}
