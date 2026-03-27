import { useEffect, useState } from 'react'
import { useParams, useSearchParams, Link } from 'react-router-dom'
import { fetchDbTables, fetchDbSchema, queryDbData } from '../api'
import type { DbTable, DbSchema, DbQueryResult } from '../types'

export function DbExplorer() {
  const { slug: envSlug, appSlug } = useParams<{ slug: string; appSlug?: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const selectedTable = searchParams.get('table')

  const [tables, setTables] = useState<DbTable[]>([])
  const [schema, setSchema] = useState<DbSchema | null>(null)
  const [queryResult, setQueryResult] = useState<DbQueryResult | null>(null)
  const [loading, setLoading] = useState(true)
  const [tableLoading, setTableLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!envSlug) return
    setLoading(true)
    setError(null)
    fetchDbTables(envSlug, appSlug)
      .then(setTables)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false))
  }, [envSlug, appSlug])

  useEffect(() => {
    if (!envSlug || !selectedTable) {
      setSchema(null)
      setQueryResult(null)
      return
    }
    setTableLoading(true)
    setError(null)
    Promise.all([fetchDbSchema(envSlug, selectedTable, appSlug), queryDbData(envSlug, selectedTable, 50, 0, appSlug)])
      .then(([s, q]) => {
        setSchema(s)
        setQueryResult(q)
      })
      .catch((e) => setError(e.message))
      .finally(() => setTableLoading(false))
  }, [envSlug, selectedTable, appSlug])

  return (
    <div className="space-y-6">
      {/* Breadcrumb + Header */}
      <div>
        <div className="flex items-center gap-2 mb-2">
          <Link to="/environments" className="text-xs text-white/30 hover:text-white/50">
            Environments
          </Link>
          <span className="text-xs text-white/15">/</span>
          <Link to={`/environments/${envSlug}`} className="text-xs text-white/30 hover:text-white/50">
            {envSlug}
          </Link>
          <span className="text-xs text-white/15">/</span>
          <span className="text-xs text-white/50">DB Explorer</span>
        </div>
        <h1 className="text-2xl font-bold text-[#e2e8f0]">DB Explorer</h1>
        <p className="text-sm text-white/40 mt-1">
          Browse database tables, schemas and data for{' '}
          <span className="text-[#a78bfa]">{envSlug}</span>
        </p>
      </div>

      {error && (
        <div className="bg-red-500/10 border border-red-500/30 rounded-xl p-4">
          <p className="text-sm text-red-400">Error: {error}</p>
        </div>
      )}

      {loading ? (
        <div className="flex items-center justify-center h-64">
          <div className="text-center">
            <div className="w-6 h-6 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin mx-auto mb-3" />
            <p className="text-white/30 text-sm">Loading tables...</p>
          </div>
        </div>
      ) : (
        <div className="flex gap-6 min-h-[500px]">
          {/* Sidebar: table list */}
          <div className="w-64 shrink-0">
            <div className="rounded-xl border border-white/5 p-4" style={{ background: '#1e1e3a' }}>
              <h2 className="text-[10px] font-medium text-white/30 uppercase tracking-wider mb-3">
                Tables ({tables.length})
              </h2>
              {tables.length === 0 ? (
                <p className="text-white/30 text-sm">No tables found</p>
              ) : (
                <ul className="space-y-1">
                  {tables.map((t) => (
                    <li key={t.name}>
                      <button
                        onClick={() => setSearchParams({ table: t.name })}
                        className={`w-full text-left px-3 py-2 rounded-lg text-sm transition ${
                          selectedTable === t.name
                            ? 'bg-[#7c3aed]/15 text-[#a78bfa] border border-[#7c3aed]/30'
                            : 'text-white/50 hover:bg-white/5 border border-transparent'
                        }`}
                      >
                        <span className="font-mono text-xs">{t.name}</span>
                        <span className="text-white/20 ml-2 text-xs">{t.row_count} rows</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>

          {/* Main content */}
          <div className="flex-1 min-w-0">
            {!selectedTable ? (
              <div className="rounded-xl border border-white/5 p-16 text-center" style={{ background: '#1e1e3a' }}>
                <p className="text-white/30">Select a table to explore its schema and data</p>
              </div>
            ) : tableLoading ? (
              <div className="flex items-center justify-center h-64">
                <div className="text-center">
                  <div className="w-6 h-6 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin mx-auto mb-3" />
                  <p className="text-white/30 text-sm">Loading table data...</p>
                </div>
              </div>
            ) : (
              <div className="space-y-6">
                {/* Schema section */}
                {schema && (
                  <div className="rounded-xl border border-white/5 p-5" style={{ background: '#1e1e3a' }}>
                    <h3 className="text-xs font-medium text-white/30 uppercase tracking-wider mb-3">
                      Schema:{' '}
                      <span className="font-mono text-[#a78bfa] normal-case">{schema.table_name}</span>
                    </h3>
                    <div className="overflow-x-auto">
                      <table className="w-full text-sm">
                        <thead>
                          <tr className="border-b border-white/10 text-white/30">
                            <th className="text-left py-2 px-3 text-[10px] uppercase tracking-wider">Column</th>
                            <th className="text-left py-2 px-3 text-[10px] uppercase tracking-wider">Type</th>
                            <th className="text-left py-2 px-3 text-[10px] uppercase tracking-wider">Nullable</th>
                            <th className="text-left py-2 px-3 text-[10px] uppercase tracking-wider">PK</th>
                            <th className="text-left py-2 px-3 text-[10px] uppercase tracking-wider">Default</th>
                          </tr>
                        </thead>
                        <tbody>
                          {schema.columns.map((col) => (
                            <tr key={col.name} className="border-b border-white/5 text-white/60">
                              <td className="py-2 px-3 font-mono text-xs">{col.name}</td>
                              <td className="py-2 px-3 text-amber-400 text-xs">{col.data_type}</td>
                              <td className="py-2 px-3 text-xs">
                                {col.nullable ? <span className="text-white/20">Yes</span> : 'No'}
                              </td>
                              <td className="py-2 px-3 text-xs">
                                {col.primary_key && <span className="text-[#a78bfa] font-medium">PK</span>}
                              </td>
                              <td className="py-2 px-3 text-white/20 text-xs">{col.default_value || '-'}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>

                    {schema.relations.length > 0 && (
                      <div className="mt-4 pt-4 border-t border-white/5">
                        <h4 className="text-[10px] font-medium text-white/30 uppercase tracking-wider mb-2">Relations</h4>
                        <ul className="space-y-1">
                          {schema.relations.map((rel, i) => (
                            <li key={i} className="text-xs font-mono text-white/50">
                              {rel.from_table}.{rel.from_column} &rarr; {rel.to_table}.{rel.to_column}
                              <span className="text-white/20 ml-2">({rel.relation_type})</span>
                            </li>
                          ))}
                        </ul>
                      </div>
                    )}
                  </div>
                )}

                {/* Data preview */}
                {queryResult && (
                  <div className="rounded-xl border border-white/5 p-5" style={{ background: '#1e1e3a' }}>
                    <div className="flex items-center justify-between mb-3">
                      <h3 className="text-xs font-medium text-white/30 uppercase tracking-wider">Data Preview</h3>
                      <span className="text-xs text-white/20">{queryResult.total_count} total rows</span>
                    </div>
                    <div className="overflow-x-auto">
                      <table className="w-full text-sm">
                        <thead>
                          <tr className="border-b border-white/10 text-white/30">
                            {queryResult.columns.map((col) => (
                              <th key={col} className="text-left py-2 px-3 font-mono text-[10px] whitespace-nowrap uppercase tracking-wider">{col}</th>
                            ))}
                          </tr>
                        </thead>
                        <tbody>
                          {queryResult.rows.map((row, i) => (
                            <tr key={i} className="border-b border-white/5 text-white/60 hover:bg-white/[0.02]">
                              {queryResult.columns.map((col) => (
                                <td key={col} className="py-2 px-3 text-xs max-w-xs truncate">
                                  {row[col] === null ? (
                                    <span className="text-white/15 italic">null</span>
                                  ) : (
                                    String(row[col])
                                  )}
                                </td>
                              ))}
                            </tr>
                          ))}
                          {queryResult.rows.length === 0 && (
                            <tr>
                              <td colSpan={queryResult.columns.length} className="py-8 text-center text-white/30 text-sm">
                                No data in this table
                              </td>
                            </tr>
                          )}
                        </tbody>
                      </table>
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
