"use client";

import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import clsx from "clsx";
import { useDebounced } from "@/lib/hooks";
import { errorMessage } from "@/lib/api";
import { controlClass } from "@/components/ui/Field";

export interface SearchPickerProps<T> {
  label?: ReactNode;
  placeholder?: string;
  /** Runs on every (debounced) keystroke, including the empty query on focus. */
  search: (q: string) => Promise<T[]>;
  getKey: (item: T) => string;
  getLabel: (item: T) => string;
  renderOption?: (item: T) => ReactNode;
  value: T[];
  onChange: (value: T[]) => void;
  multiple?: boolean;
  /** Shown when a search returns nothing. Explain the rule, not just "no results". */
  emptyMessage: ReactNode;
  error?: string;
  hint?: ReactNode;
  disabled?: boolean;
  required?: boolean;
}

/**
 * Debounced search box with a dropdown of results. Selected items become chips
 * (multiple) or fill the input (single). All lookups go through `search`, so the
 * picker itself never knows which endpoint it is bound to.
 */
export function SearchPicker<T>({
  label,
  placeholder = "Type to search",
  search,
  getKey,
  getLabel,
  renderOption,
  value,
  onChange,
  multiple = false,
  emptyMessage,
  error,
  hint,
  disabled,
  required,
}: SearchPickerProps<T>) {
  const id = useId();
  const [query, setQuery] = useState("");
  const debounced = useDebounced(query, 250);
  const [open, setOpen] = useState(false);
  const [results, setResults] = useState<T[]>([]);
  const [loading, setLoading] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [highlight, setHighlight] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const searchRef = useRef(search);
  searchRef.current = search;

  useEffect(() => {
    if (!open) return;
    let active = true;
    setLoading(true);
    setSearchError(null);
    searchRef
      .current(debounced)
      .then((items) => {
        if (!active) return;
        setResults(items);
        setHighlight(0);
      })
      .catch((err: unknown) => {
        if (!active) return;
        setResults([]);
        setSearchError(errorMessage(err));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [debounced, open]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const selectedKeys = new Set(value.map(getKey));
  const visible = results.filter((r) => !selectedKeys.has(getKey(r)));

  function select(item: T) {
    if (multiple) {
      onChange([...value, item]);
      setQuery("");
    } else {
      onChange([item]);
      setQuery("");
      setOpen(false);
    }
  }

  function remove(item: T) {
    const key = getKey(item);
    onChange(value.filter((v) => getKey(v) !== key));
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (!open && (e.key === "ArrowDown" || e.key === "Enter")) {
      setOpen(true);
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setHighlight((h) => Math.min(visible.length - 1, h + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setHighlight((h) => Math.max(0, h - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = visible[highlight];
      if (item) select(item);
    } else if (e.key === "Escape") {
      setOpen(false);
    } else if (e.key === "Backspace" && query === "" && value.length > 0 && multiple) {
      const last = value[value.length - 1];
      if (last) remove(last);
    }
  }

  const single = !multiple ? value[0] : undefined;

  return (
    <div className="flex flex-col gap-1" ref={root}>
      {label ? (
        <label htmlFor={id} className="text-sm font-medium text-slate-700">
          {label}
          {required ? <span className="ml-0.5 text-red-600">*</span> : null}
        </label>
      ) : null}
      <div className="relative">
        <div
          className={clsx(
            controlClass,
            error ? "border-red-400" : "border-slate-300",
            "flex min-h-[2.5rem] flex-wrap items-center gap-1 py-1",
          )}
        >
          {multiple
            ? value.map((item) => (
                <span
                  key={getKey(item)}
                  className="inline-flex items-center gap-1 rounded-full bg-accent-50 px-2 py-0.5 text-xs font-medium text-accent-800 ring-1 ring-inset ring-accent-200"
                >
                  {getLabel(item)}
                  <button
                    type="button"
                    onClick={() => remove(item)}
                    className="rounded-full px-1 hover:bg-accent-100"
                    aria-label={`Remove ${getLabel(item)}`}
                    disabled={disabled}
                  >
                    x
                  </button>
                </span>
              ))
            : null}
          <input
            id={id}
            type="text"
            role="combobox"
            aria-expanded={open}
            aria-controls={`${id}-listbox`}
            aria-autocomplete="list"
            disabled={disabled}
            className="min-w-[8rem] flex-1 border-0 bg-transparent p-0 text-sm focus:outline-none focus:ring-0"
            placeholder={single ? getLabel(single) : placeholder}
            value={query}
            onFocus={() => setOpen(true)}
            onChange={(e) => {
              setQuery(e.target.value);
              setOpen(true);
            }}
            onKeyDown={onKeyDown}
          />
          {single && !multiple ? (
            <button
              type="button"
              onClick={() => onChange([])}
              className="text-xs text-slate-500 hover:text-slate-800"
              aria-label="Clear selection"
              disabled={disabled}
            >
              Clear
            </button>
          ) : null}
        </div>
        {open ? (
          <div
            id={`${id}-listbox`}
            role="listbox"
            className="absolute z-30 mt-1 max-h-64 w-full overflow-y-auto rounded-md border border-slate-200 bg-white py-1 shadow-lg"
          >
            {loading && visible.length === 0 ? (
              <p className="px-3 py-2 text-sm text-slate-500">Searching</p>
            ) : searchError ? (
              <p className="px-3 py-2 text-sm text-red-700">{searchError}</p>
            ) : visible.length === 0 ? (
              <div className="px-3 py-2 text-sm text-slate-600">{emptyMessage}</div>
            ) : (
              visible.map((item, i) => (
                <button
                  key={getKey(item)}
                  type="button"
                  role="option"
                  aria-selected={i === highlight}
                  onMouseEnter={() => setHighlight(i)}
                  onClick={() => select(item)}
                  className={clsx(
                    "block w-full px-3 py-2 text-left text-sm",
                    i === highlight ? "bg-accent-50 text-accent-900" : "text-slate-800 hover:bg-slate-50",
                  )}
                >
                  {renderOption ? renderOption(item) : getLabel(item)}
                </button>
              ))
            )}
          </div>
        ) : null}
      </div>
      {error ? (
        <p className="text-xs text-red-600" role="alert">
          {error}
        </p>
      ) : hint ? (
        <p className="text-xs text-slate-500">{hint}</p>
      ) : null}
    </div>
  );
}
