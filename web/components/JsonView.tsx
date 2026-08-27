export function JsonView({ value }: { value: unknown }) {
  if (value === null || value === undefined) return <span className="text-xs text-slate-400">none</span>;
  let text: string;
  try {
    text = JSON.stringify(value, null, 2);
  } catch {
    text = String(value);
  }
  return (
    <pre className="max-h-64 overflow-auto rounded-md bg-slate-900 p-3 text-xs leading-relaxed text-slate-100">
      {text}
    </pre>
  );
}
