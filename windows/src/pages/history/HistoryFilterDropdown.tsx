import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import type { FilterOption } from "./HistoryViewTypes";

export function FilterDropdown({
  value,
  options,
  label,
  testId,
  onChange,
}: {
  value: string;
  options: FilterOption[];
  label: string;
  testId: string;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const selected = options.find((option) => option.value === value) ?? options[0];
  const SelectedIcon = selected?.icon;

  useEffect(() => {
    if (!open) return;
    const closeOnPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", closeOnPointerDown);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnPointerDown);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  return (
    <div className="filter-dropdown" ref={rootRef} data-testid={testId}>
      <button
        type="button"
        className={`filter-trigger${open ? " is-open" : ""}${value !== "all" ? " is-filtered" : ""}`}
        aria-label={`${label}: ${selected?.label}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        {SelectedIcon && (
          <SelectedIcon
            className={
              selected.category
                ? `filter-category-icon ${selected.category}`
                : undefined
            }
            size={13}
            strokeWidth={1.8}
            aria-hidden="true"
          />
        )}
        <span>{selected?.label}</span>
        <ChevronDown size={13} strokeWidth={1.8} aria-hidden="true" />
      </button>
      {open && (
        <div className="filter-menu" role="listbox" aria-label={label}>
          {options.map((option) => {
            const OptionIcon = option.icon;
            const selectedOption = option.value === value;
            return (
              <button
                type="button"
                className={`filter-option${selectedOption ? " is-selected" : ""}`}
                role="option"
                aria-selected={selectedOption}
                key={option.value}
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                }}
              >
                {OptionIcon ? (
                  <OptionIcon
                    className={
                      option.category
                        ? `filter-category-icon ${option.category}`
                        : undefined
                    }
                    size={13}
                    strokeWidth={1.8}
                    aria-hidden="true"
                  />
                ) : (
                  <span className="filter-option-spacer" />
                )}
                <span>{option.label}</span>
                {selectedOption && <Check size={13} strokeWidth={2} aria-hidden="true" />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
