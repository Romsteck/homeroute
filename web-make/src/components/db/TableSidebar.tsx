import { useState, useEffect } from 'react'
import type { DbTable, EnvApp } from '../../types'

interface AppWithTables {
  app: EnvApp
  tables: DbTable[]
}

interface TableSidebarProps {
  appsWithTables: AppWithTables[]
  loading: boolean
  selectedAppSlug: string | null
  selectedTable: string | null
  onSelectTable: (appSlug: string, tableName: string) => void
}

export function TableSidebar({
  appsWithTables,
  loading,
  selectedAppSlug,
  selectedTable,
  onSelectTable,
}: TableSidebarProps) {
  const [expandedApps, setExpandedApps] = useState<Set<string>>(() => {
    // Auto-expand the app that has the selected table
    if (selectedAppSlug) return new Set([selectedAppSlug])
    // Auto-expand all if few apps
    if (appsWithTables.length <= 3) return new Set(appsWithTables.map((a) => a.app.slug))
    return new Set()
  })

  useEffect(() => {
    if (appsWithTables.length === 0) return
    setExpandedApps((prev) => {
      if (prev.size > 0) return prev
      if (selectedAppSlug) return new Set([selectedAppSlug])
      if (appsWithTables.length <= 3) return new Set(appsWithTables.map((a) => a.app.slug))
      return prev
    })
  }, [appsWithTables, selectedAppSlug])

  function toggleApp(slug: string) {
    setExpandedApps((prev) => {
      const next = new Set(prev)
      if (next.has(slug)) next.delete(slug)
      else next.add(slug)
      return next
    })
  }

  const totalTables = appsWithTables.reduce((sum, a) => sum + a.tables.length, 0)

  return (
    <div className="w-60 shrink-0 flex flex-col overflow-hidden rounded-lg border border-white/5" style={{ background: '#1e1e3a' }}>
      <div className="px-4 py-3 border-b border-white/5">
        <h2 className="text-[10px] font-medium text-white/30 uppercase tracking-wider">
          Tables ({totalTables})
        </h2>
      </div>

      <div className="flex-1 overflow-y-auto px-2 py-2">
        {loading ? (
          <div className="flex items-center justify-center py-8">
            <div className="w-5 h-5 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin" />
          </div>
        ) : appsWithTables.length === 0 ? (
          <p className="text-white/20 text-xs px-2 py-4">No apps with DB found</p>
        ) : (
          <div className="space-y-0.5">
            {appsWithTables.map(({ app, tables }) => {
              const isExpanded = expandedApps.has(app.slug)
              return (
                <div key={app.slug}>
                  {/* App header */}
                  <button
                    onClick={() => toggleApp(app.slug)}
                    className="w-full flex items-center gap-2 px-2 py-2 rounded-md text-left hover:bg-white/5 transition-colors"
                  >
                    <svg
                      className={`w-3 h-3 text-white/20 transition-transform shrink-0 ${isExpanded ? 'rotate-90' : ''}`}
                      fill="none"
                      viewBox="0 0 24 24"
                      stroke="currentColor"
                      strokeWidth={2}
                    >
                      <path strokeLinecap="round" strokeLinejoin="round" d="M8.25 4.5l7.5 7.5-7.5 7.5" />
                    </svg>
                    <span className={`w-2 h-2 rounded-full shrink-0 ${app.running ? 'bg-emerald-400' : 'bg-slate-500'}`} />
                    <span className="text-xs font-medium text-white/60 truncate">{app.name}</span>
                    <span className="text-[10px] text-white/15 ml-auto shrink-0">{tables.length}</span>
                  </button>

                  {/* Tables list */}
                  {isExpanded && (
                    <div className="ml-5 space-y-0.5 pb-1">
                      {tables.map((t) => {
                        const isActive = selectedAppSlug === app.slug && selectedTable === t.name
                        return (
                          <button
                            key={t.name}
                            onClick={() => onSelectTable(app.slug, t.name)}
                            className={`w-full text-left px-2.5 py-1.5 rounded-md text-xs transition-colors ${
                              isActive
                                ? 'bg-[#7c3aed]/15 text-[#a78bfa] border border-[#7c3aed]/30'
                                : 'text-white/40 hover:bg-white/5 hover:text-white/60 border border-transparent'
                            }`}
                          >
                            <div className="flex items-center justify-between gap-2">
                              <span className="font-mono truncate">{t.name}</span>
                              <span className="text-[10px] text-white/15 shrink-0">{t.row_count}</span>
                            </div>
                          </button>
                        )
                      })}
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
