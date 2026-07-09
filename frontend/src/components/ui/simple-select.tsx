"use client";

import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from '@/components/ui/select';

export interface SimpleSelectOption {
  value: string;
  label: string;
}

/**
 * Thin convenience wrapper over the Radix Select composition for the common
 * "flat list of string options" case — the drop-in replacement for a native
 * <select>, but portal-rendered and collision-aware so it can't be clipped
 * by an overflow ancestor (the recurring dropdown bug this replaced).
 * Compose ui/select directly when you need groups, custom item content, etc.
 */
export function SimpleSelect({
  value,
  onValueChange,
  options,
  placeholder,
  disabled,
  className,
  title,
}: {
  value: string;
  onValueChange: (value: string) => void;
  options: SimpleSelectOption[];
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  title?: string;
}) {
  return (
    <Select value={value} onValueChange={onValueChange} disabled={disabled}>
      <SelectTrigger className={className} title={title}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {options.map((opt) => (
          <SelectItem key={opt.value} value={opt.value}>
            {opt.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
