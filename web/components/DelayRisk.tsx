import { Badge, type BadgeTone } from "./ui/Badge";
import { formatPercent } from "@/lib/format";

/** Thresholds for the 0 to 1 score the analytics service returns. */
export function delayRiskBand(value: number): { tone: BadgeTone; label: string } {
  if (value >= 0.66) return { tone: "danger", label: "High" };
  if (value >= 0.33) return { tone: "warning", label: "Medium" };
  return { tone: "success", label: "Low" };
}

export function DelayRisk({ value }: { value: string | number | null | undefined }) {
  if (value === null || value === undefined || value === "") {
    return <span className="text-xs text-slate-400">Not scored</span>;
  }
  const n = typeof value === "number" ? value : Number(value);
  if (Number.isNaN(n)) return <span className="text-xs text-slate-400">Not scored</span>;
  const band = delayRiskBand(n);
  return (
    <Badge tone={band.tone} title={`Delay risk ${n}`}>
      {band.label} risk, {formatPercent(n)}
    </Badge>
  );
}
