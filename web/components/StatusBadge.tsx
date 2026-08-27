import { Badge, type BadgeTone } from "./ui/Badge";
import { humanize } from "@/lib/format";

/**
 * One colour map for every status string in the platform, so a "delivered"
 * shipment, a "paid" invoice and a "done" work order all read as the same kind of
 * good news wherever they appear.
 */
export const STATUS_TONES: Record<string, BadgeTone> = {
  // neutral: not started, drafts, pending decisions
  draft: "neutral",
  open: "neutral",
  planned: "neutral",
  scheduled: "neutral",
  pending: "neutral",
  received: "neutral",
  submitted: "neutral",
  current: "neutral",
  low: "neutral",
  normal: "neutral",
  // info: moving through the process
  booked: "info",
  triaged: "info",
  in_progress: "info",
  picked_up: "info",
  in_transit: "info",
  customs: "info",
  out_for_delivery: "info",
  in_use: "info",
  issued: "info",
  approved: "info",
  pending_approval: "info",
  manager_approved: "info",
  partially_paid: "info",
  on_leave: "info",
  high: "info",
  // success: finished well
  delivered: "success",
  done: "success",
  completed: "success",
  resolved: "success",
  closed: "success",
  paid: "success",
  active: "success",
  available: "success",
  finance_approved: "success",
  posted: "success",
  cleared: "success",
  // warning: needs attention
  exception: "warning",
  blocked: "warning",
  waiting_on_requester: "warning",
  on_hold: "warning",
  late: "warning",
  missed: "warning",
  suspended: "warning",
  maintenance: "warning",
  locked: "warning",
  urgent: "warning",
  // danger: stopped or refused
  cancelled: "danger",
  rejected: "danger",
  void: "danger",
  terminated: "danger",
  failed: "danger",
  disabled: "danger",
  retired: "danger",
  breached: "danger",
};

export function statusTone(status: string | null | undefined): BadgeTone {
  if (!status) return "neutral";
  return STATUS_TONES[status] ?? "neutral";
}

export function StatusBadge({
  status,
  label,
  className,
}: {
  status: string | null | undefined;
  label?: string;
  className?: string;
}) {
  if (!status) return null;
  return (
    <Badge tone={statusTone(status)} className={className} title={status}>
      {label ?? humanize(status)}
    </Badge>
  );
}
