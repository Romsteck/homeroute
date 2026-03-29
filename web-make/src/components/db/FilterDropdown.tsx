import { useState, useRef, useEffect } from 'react'
import type { DbFilter, DbColumn } from '../../types'

const OPERATORS: { value: DbFilter['op']; label: string; needsValue: boolean }[] = [
  { value: 'eq', label: 'Equals', needsValue: true },
  { value: 'ne', label: 'Not equals', needsValue: true },
  { value: 'gt', label: 'Greater than', needsValue: true },
  { value: 'lt', label: 'Less than', needsValue: true },
  { value: 'gte', label: 'Greater or equal', needsValue: true },
  { value: 'lte', label: 'Less or equal', needsValue: true },
  { value: 'like', label: 'Contains', needsValue: true },
  { value: 'is_null', label: 'Is null', needsValue: false },
  { value: 'is_not_null', label: 'Is not null', needsValue: false },
]

interface FilterDropdownProps {
  column: DbColumn
  currentFilter?: DbFilter
  onApply: (filter: DbFilter | null) => void
  onClose: () => void
}

export function FilterDropdown({ column, currentFilter, onApply, onClose }: FilterDropdownProps) {
  const [op, setOp] = useState<DbFilter['op']>(currentFilter?.op || 'eq')
  const [value, setValue] = useState(currentFilter?.value?.toString() || '')
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose()
      }
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [onClose])

  const selectedOp = OPERATORS.find((o) => o.value === op)
  const needsValue = selectedOp?.needsValue !== false

  function handleApply() {
    if (needsValue && !value.trim()) {
      onApply(null)
    } else {
      onApply({ column: column.name, op, value: needsValue ? value : null })
    }
    onClose()
  }

  function handleClear() {
    onApply(null)
    onClose()
  }

  return (
    <div
      ref={ref}
      className="absolute top-full left-0 mt-1 z-50 w-64 rounded-lg border border-white/10 shadow-xl p-3 space-y-3"
      style={{ background: '#252547' }}
    >
      <div className="text-xs font-medium text-white/50 truncate">
        Filter: <span className="text-[#a78bfa] font-mono">{column.name}</span>
      </div>

      {/* Operator */}
      <select
        value={op}
        onChange={(e) => setOp(e.target.value as DbFilter['op'])}
        className="w-full text-sm rounded-md border border-white/10 bg-white/5 text-white/80 px-2 py-1.5 focus:outline-none focus:border-[#7c3aed]/50"
      >
        {OPERATORS.map((o) => (
          <option key={o.value} value={o.value} className="bg-[#1e1e3a] text-white">
            {o.label}
          </option>
        ))}
      </select>

      {/* Value input */}
      {needsValue && (
        <input
          type="text"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleApply()}
          placeholder="Value..."
          autoFocus
          className="w-full text-sm rounded-md border border-white/10 bg-white/5 text-white/80 px-2 py-1.5 placeholder-white/20 focus:outline-none focus:border-[#7c3aed]/50"
        />
      )}

      {/* Buttons */}
      <div className="flex items-center gap-2">
        <button
          onClick={handleApply}
          className="flex-1 text-xs font-medium px-3 py-1.5 rounded-md bg-[#7c3aed] text-white hover:bg-[#6d28d9] transition-colors"
        >
          Apply
        </button>
        {currentFilter && (
          <button
            onClick={handleClear}
            className="text-xs font-medium px-3 py-1.5 rounded-md bg-white/5 text-white/50 hover:bg-white/10 transition-colors"
          >
            Clear
          </button>
        )}
        <button
          onClick={onClose}
          className="text-xs font-medium px-3 py-1.5 rounded-md bg-white/5 text-white/50 hover:bg-white/10 transition-colors"
        >
          Cancel
        </button>
      </div>
    </div>
  )
}
