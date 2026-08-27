import clsx from "clsx";
import { initials } from "@/lib/format";

export function Avatar({ name, size = "md" }: { name: string; size?: "sm" | "md" | "lg" }) {
  const cls = size === "sm" ? "h-7 w-7 text-xs" : size === "lg" ? "h-12 w-12 text-base" : "h-9 w-9 text-sm";
  return (
    <span
      aria-hidden="true"
      className={clsx(
        "inline-flex shrink-0 items-center justify-center rounded-full bg-accent-100 font-semibold text-accent-800",
        cls,
      )}
    >
      {initials(name) || "?"}
    </span>
  );
}
