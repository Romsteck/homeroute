import { useState, useCallback } from 'react'
import { FilterDropdown } from './FilterDropdown'
import type { DbColumn, DbFilter, DbQueryResult } from '../../types'

interface DataGridProps {
  result: DbQueryResult
  schema: DbColumn[]
  filters: DbFilter[]
  sortColumn: string | null
  sortDesc: boolean
  selectedRows: Set<number>
  editingCell: { row: number; col: string } | null
  editValue: string
  savingCell: { row: number; col: string } | null
  onSort: (column: string) => void
  onFilterChange: (column: string, filter: DbFilter | null) => void
  onSelectRow: (rowIndex: number, checked: boolean) => void
  onSelectAll: (checked: boolean) => void
  onStartEdit: (row: number, col: string, value: any) => void
  onEditChange: (value: string) => void
  onCommitEdit: () => void
  onCancelEdit: () => void
}

export function DataGrid({
  result,
  schema,
  filters,
  sortColumn,
  sortDesc,
  selectedRows,
  editingCell,
  editValue,
  savingCell,
  onSort,
  onFilterChange,
  onSelectRow,
  onSelectAll,
  onStartEdit,
  onEditChange,
  onCommitEdit,
  onCancelEdit,
}: DataGridProps) {
  const [filterOpen, setFilterOpen] = useState<string | null>(null)

  const allSelected = result.rows.length > 0 && selectedRows.size === result.rows.length

  const getColumnSchema = useCallback(
    (name: string) => schema.find((c) => c.name === name),
    [schema],
  )

  const getFilterForColumn = useCallback(
    (name: string) => filters.find((f) => f.column === name),
    [filters],
  )

  function renderCellValue(value: any, colName: string) {
    if (value === null || value === undefined) {
      return <span className="text-white/15 italic text-xs">null</span>
    }

    const strVal = String(value)
    if (strVal === '') {
      return <span className="text-white/20">-</span>
    }

    const colSchema = getColumnSchema(colName)
    const dt = (colSchema?.data_type || '').toLowerCase()

    // Boolean
    if (dt === 'boolean' || dt === 'bool') {
      const boolVal = value === true || value === 1 || value === 'true' || value === '1'
      return (
        <span className={`inline-flex items-center gap-1 text-xs ${boolVal ? 'text-emerald-400' : 'text-white/30'}`}>
          <span className={`w-3 h-3 rounded border ${boolVal ? 'bg-[#7c3aed] border-[#7c3aed]' : 'border-white/20 bg-transparent'} flex items-center justify-center`}>
            {boolVal && (
              <svg className="w-2 h-2 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 12.75l6 6 9-13.5" />
              </svg>
            )}
          </span>
          {boolVal ? 'true' : 'false'}
        </span>
      )
    }

    // Number — right align handled by parent
    if (dt === 'integer' || dt === 'int' || dt === 'bigint' || dt === 'real' || dt === 'float' || dt === 'numeric' || dt === 'decimal') {
      return <span className="font-mono text-xs">{strVal}</span>
    }

    // URL detection
    if (typeof value === 'string' && /^https?:\/\//i.test(value)) {
      return (
        <a
          href={value}
          target="_blank"
          rel="noopener noreferrer"
          className="text-[#a78bfa] hover:underline text-xs truncate block max-w-[200px]"
          title={value}
        >
          {value}
        </a>
      )
    }

    // Email detection
    if (typeof value === 'string' && /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value)) {
      return (
        <a href={`mailto:${value}`} className="text-[#a78bfa] hover:underline text-xs">
          {value}
        </a>
      )
    }

    // DateTime detection
    if (typeof value === 'string' && /^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}/.test(value)) {
      try {
        const d = new Date(value)
        return (
          <span className="text-xs text-white/60" title={value}>
            {d.toLocaleDateString()} {d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
          </span>
        )
      } catch {
        // fall through
      }
    }

    return <span className="text-xs truncate block max-w-[300px]" title={strVal}>{strVal}</span>
  }

  function isNumberType(colName: string): boolean {
    const col = getColumnSchema(colName)
    if (!col) return false
    const dt = col.data_type.toLowerCase()
    return ['integer', 'int', 'bigint', 'smallint', 'real', 'float', 'double', 'numeric', 'decimal'].includes(dt)
  }

  return (
    <div className="overflow-x-auto rounded-lg border border-white/5" style={{ background: '#1e1e3a' }}>
      <table className="w-full text-sm">
        <thead className="sticky top-0 z-10" style={{ background: '#1e1e3a' }}>
          <tr className="border-b border-white/10">
            {/* Checkbox column */}
            <th className="w-10 px-3 py-2.5">
              <input
                type="checkbox"
                checked={allSelected}
                onChange={(e) => onSelectAll(e.target.checked)}
                className="w-3.5 h-3.5 rounded border-white/20 bg-white/5 accent-[#7c3aed]"
              />
            </th>

            {result.columns.map((col) => {
              const isSorted = sortColumn === col
              const hasFilter = !!getFilterForColumn(col)

              return (
                <th key={col} className="relative px-3 py-2.5 text-left">
                  <div className="flex items-center gap-1">
                    <button
                      onClick={() => onSort(col)}
                      className={`flex items-center gap-1 text-[11px] font-medium uppercase tracking-wider transition-colors ${
                        isSorted ? 'text-[#a78bfa]' : 'text-white/30 hover:text-white/50'
                      }`}
                    >
                      <span className="truncate max-w-[150px]">{col}</span>
                      {isSorted && (
                        <svg className="w-3 h-3 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                          {sortDesc ? (
                            <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
                          ) : (
                            <path strokeLinecap="round" strokeLinejoin="round" d="M5 15l7-7 7 7" />
                          )}
                        </svg>
                      )}
                    </button>

                    {/* Filter icon */}
                    <button
                      onClick={() => setFilterOpen(filterOpen === col ? null : col)}
                      className={`p-0.5 rounded transition-colors shrink-0 ${
                        hasFilter
                          ? 'text-[#7c3aed] bg-[#7c3aed]/10'
                          : 'text-white/15 hover:text-white/30'
                      }`}
                      title="Filter"
                    >
                      <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                        <path strokeLinecap="round" strokeLinejoin="round" d="M12 3c2.755 0 5.455.232 8.083.678.533.09.917.556.917 1.096v1.044a2.25 2.25 0 01-.659 1.591l-5.432 5.432a2.25 2.25 0 00-.659 1.591v2.927a2.25 2.25 0 01-1.244 2.013L9.75 21v-6.568a2.25 2.25 0 00-.659-1.591L3.659 7.409A2.25 2.25 0 013 5.818V4.774c0-.54.384-1.006.917-1.096A48.32 48.32 0 0112 3z" />
                      </svg>
                    </button>
                  </div>

                  {/* Filter dropdown */}
                  {filterOpen === col && (
                    <FilterDropdown
                      column={getColumnSchema(col) || { name: col, data_type: 'text', nullable: true, primary_key: false }}
                      currentFilter={getFilterForColumn(col)}
                      onApply={(f) => onFilterChange(col, f)}
                      onClose={() => setFilterOpen(null)}
                    />
                  )}
                </th>
              )
            })}
          </tr>
        </thead>

        <tbody>
          {result.rows.length === 0 ? (
            <tr>
              <td colSpan={result.columns.length + 1} className="py-16 text-center text-white/25 text-sm">
                No data found
              </td>
            </tr>
          ) : (
            result.rows.map((row, rowIdx) => {
              const isSelected = selectedRows.has(rowIdx)
              return (
                <tr
                  key={rowIdx}
                  className={`border-b border-white/[0.03] transition-colors ${
                    isSelected
                      ? 'bg-[#7c3aed]/5'
                      : rowIdx % 2 === 0
                        ? 'bg-transparent hover:bg-white/[0.02]'
                        : 'bg-white/[0.015] hover:bg-white/[0.03]'
                  }`}
                >
                  <td className="w-10 px-3 py-2">
                    <input
                      type="checkbox"
                      checked={isSelected}
                      onChange={(e) => onSelectRow(rowIdx, e.target.checked)}
                      className="w-3.5 h-3.5 rounded border-white/20 bg-white/5 accent-[#7c3aed]"
                    />
                  </td>

                  {result.columns.map((col) => {
                    const isEditing = editingCell?.row === rowIdx && editingCell?.col === col
                    const isSaving = savingCell?.row === rowIdx && savingCell?.col === col
                    const isNum = isNumberType(col)

                    return (
                      <td
                        key={col}
                        className={`px-3 py-2 ${isNum ? 'text-right' : ''} ${isSaving ? 'opacity-50' : ''}`}
                        onDoubleClick={() => onStartEdit(rowIdx, col, row[col])}
                      >
                        {isEditing ? (
                          <input
                            type="text"
                            value={editValue}
                            onChange={(e) => onEditChange(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === 'Enter') onCommitEdit()
                              if (e.key === 'Escape') onCancelEdit()
                            }}
                            onBlur={onCommitEdit}
                            autoFocus
                            className="w-full text-xs rounded border border-[#7c3aed]/50 bg-[#7c3aed]/10 text-white/90 px-1.5 py-0.5 focus:outline-none"
                          />
                        ) : (
                          renderCellValue(row[col], col)
                        )}
                      </td>
                    )
                  })}
                </tr>
              )
            })
          )}
        </tbody>
      </table>
    </div>
  )
}
