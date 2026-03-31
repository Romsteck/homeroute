import { useEffect, useState } from 'react'
import { useParams, Link } from 'react-router-dom'
import { fetchDbTables } from '../api'
import type { DbTable } from '../types'

export function FormBuilder() {
  const { slug: envSlug, appSlug } = useParams<{ slug: string; appSlug: string }>()
  const [tables, setTables] = useState<DbTable[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    if (!envSlug) return
    setLoading(true)
    fetchDbTables(envSlug, appSlug)
      .then(setTables)
      .catch(() => setTables([]))
      .finally(() => setLoading(false))
  }, [envSlug, appSlug])

  return (
    <div className="space-y-6">
      {/* Breadcrumb + Header */}
      <div>
        <div className="flex items-center gap-2 mb-2">
          <Link to="/tables" className="text-xs text-white/30 hover:text-white/50">
            Tables
          </Link>
          <span className="text-xs text-white/15">/</span>
          <span className="text-xs text-white/50">Form Builder</span>
        </div>
        <h1 className="text-2xl font-bold text-[#e2e8f0]">Form Builder</h1>
        <p className="text-sm text-white/40 mt-1">
          Model-driven form builder for{' '}
          <span className="text-[#a78bfa]">{appSlug || envSlug}</span>
        </p>
      </div>

      {/* Coming soon banner */}
      <div className="rounded-xl border border-[#7c3aed]/20 p-8 text-center" style={{ background: '#7c3aed10' }}>
        <h2 className="text-lg font-semibold text-[#e2e8f0] mb-2">
          Model-Driven Form Builder
        </h2>
        <p className="text-sm text-white/40 max-w-lg mx-auto">
          Build forms visually from your database tables. The form builder will auto-generate
          input fields, validation rules, and layouts based on your Dataverse schema. Create
          CRUD interfaces, custom views, and data-entry forms without writing code.
        </p>
        <p className="text-xs text-white/20 mt-4">Coming soon</p>
      </div>

      {/* Table preview */}
      <section>
        <h2 className="text-xs font-medium text-white/30 uppercase tracking-wider mb-3">
          Available Tables
        </h2>
        {loading ? (
          <div className="flex items-center justify-center h-32">
            <div className="text-center">
              <div className="w-6 h-6 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin mx-auto mb-3" />
              <p className="text-white/30 text-sm">Loading tables...</p>
            </div>
          </div>
        ) : tables.length === 0 ? (
          <div className="rounded-xl border border-white/5 p-8 text-center" style={{ background: '#1e1e3a' }}>
            <p className="text-sm text-white/30">
              No tables found. Deploy an app with a Dataverse database to get started.
            </p>
          </div>
        ) : (
          <div className="rounded-xl border border-white/5 overflow-hidden" style={{ background: '#1e1e3a' }}>
            <table className="w-full">
              <thead>
                <tr className="border-b border-white/10">
                  <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Table</th>
                  <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Rows</th>
                  <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Columns</th>
                  <th className="text-right px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Status</th>
                </tr>
              </thead>
              <tbody>
                {tables.map((t) => (
                  <tr key={t.name} className="border-b border-white/5 opacity-60">
                    <td className="px-4 py-3">
                      <span className="font-mono text-sm text-[#e2e8f0]">{t.name}</span>
                    </td>
                    <td className="px-4 py-3">
                      <span className="text-sm text-white/50">{t.row_count}</span>
                    </td>
                    <td className="px-4 py-3">
                      <span className="text-sm text-white/50">{t.column_count}</span>
                    </td>
                    <td className="px-4 py-3 text-right">
                      <span className="text-xs text-white/20">Coming soon</span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  )
}
