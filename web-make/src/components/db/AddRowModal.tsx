import { useState } from 'react'
import type { DbColumn } from '../../types'

interface AddRowModalProps {
  columns: DbColumn[]
  onSubmit: (row: Record<string, any>) => Promise<void>
  onClose: () => void
}

export function AddRowModal({ columns, onSubmit, onClose }: AddRowModalProps) {
  // Skip auto-increment / rowid PKs
  const editableColumns = columns.filter(
    (c) =>
      !(c.primary_key && c.default_value?.toLowerCase()?.includes('autoincrement')) &&
      !(c.primary_key && c.data_type.toLowerCase() === 'integer' && !c.default_value),
  )

  const [values, setValues] = useState<Record<string, string>>(() => {
    const init: Record<string, string> = {}
    for (const col of editableColumns) {
      init[col.name] = col.default_value || ''
    }
    return init
  })
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  function handleChange(colName: string, val: string) {
    setValues((prev) => ({ ...prev, [colName]: val }))
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setSubmitting(true)
    setError(null)
    try {
      const row: Record<string, any> = {}
      for (const col of editableColumns) {
        const raw = values[col.name]
        if (raw === '' && col.nullable) {
          row[col.name] = null
        } else if (raw === '') {
          continue // skip empty non-nullable (let DB handle defaults)
        } else {
          row[col.name] = coerceValue(raw, col.data_type)
        }
      }
      await onSubmit(row)
      onClose()
    } catch (err: any) {
      setError(err.message || 'Failed to insert row')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm" onClick={onClose}>
      <div
        className="w-full max-w-lg max-h-[80vh] overflow-y-auto rounded-xl border border-white/10 shadow-2xl p-6"
        style={{ background: '#1e1e3a' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-5">
          <h2 className="text-lg font-semibold text-white/90">Add Row</h2>
          <button onClick={onClose} className="text-white/30 hover:text-white/60 transition-colors">
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {error && (
          <div className="mb-4 bg-red-500/10 border border-red-500/30 rounded-lg px-3 py-2">
            <p className="text-sm text-red-400">{error}</p>
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-3">
          {editableColumns.map((col) => (
            <div key={col.name}>
              <label className="flex items-center gap-2 text-xs font-medium text-white/50 mb-1">
                <span className="font-mono">{col.name}</span>
                <span className="text-white/20">{col.data_type}</span>
                {col.nullable && <span className="text-white/15 italic">nullable</span>}
              </label>
              {isBooleanType(col.data_type) ? (
                <select
                  value={values[col.name]}
                  onChange={(e) => handleChange(col.name, e.target.value)}
                  className="w-full text-sm rounded-md border border-white/10 bg-white/5 text-white/80 px-3 py-2 focus:outline-none focus:border-[#7c3aed]/50"
                >
                  <option value="" className="bg-[#1e1e3a]">--</option>
                  <option value="true" className="bg-[#1e1e3a]">true</option>
                  <option value="false" className="bg-[#1e1e3a]">false</option>
                </select>
              ) : (
                <input
                  type="text"
                  value={values[col.name]}
                  onChange={(e) => handleChange(col.name, e.target.value)}
                  placeholder={col.nullable ? 'null' : ''}
                  className="w-full text-sm rounded-md border border-white/10 bg-white/5 text-white/80 px-3 py-2 placeholder-white/15 focus:outline-none focus:border-[#7c3aed]/50"
                />
              )}
            </div>
          ))}

          <div className="flex justify-end gap-3 pt-3 border-t border-white/5">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 text-sm rounded-lg bg-white/5 text-white/50 hover:bg-white/10 transition-colors"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={submitting}
              className="px-4 py-2 text-sm rounded-lg bg-[#7c3aed] text-white hover:bg-[#6d28d9] disabled:opacity-50 transition-colors"
            >
              {submitting ? 'Inserting...' : 'Insert Row'}
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}

function isBooleanType(dt: string): boolean {
  const lower = dt.toLowerCase()
  return lower === 'boolean' || lower === 'bool'
}

function coerceValue(raw: string, dataType: string): any {
  const lower = dataType.toLowerCase()
  if (lower === 'integer' || lower === 'int' || lower === 'bigint' || lower === 'smallint') {
    const n = parseInt(raw, 10)
    return isNaN(n) ? raw : n
  }
  if (lower === 'real' || lower === 'float' || lower === 'double' || lower === 'numeric' || lower === 'decimal') {
    const n = parseFloat(raw)
    return isNaN(n) ? raw : n
  }
  if (isBooleanType(lower)) {
    return raw === 'true' || raw === '1'
  }
  return raw
}
