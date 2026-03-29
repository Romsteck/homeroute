import { useEffect, useState, useCallback, useRef } from 'react'
import { useParams, useSearchParams, Link } from 'react-router-dom'
import {
  fetchEnvironments,
  fetchDbTables,
  fetchDbSchema,
  queryDbData,
  insertDbRows,
  updateDbRows,
  deleteDbRows,
} from '../api'
import type { DbTable, DbSchema, DbQueryResult, DbFilter, EnvApp, Environment } from '../types'
import { TableSidebar } from '../components/db/TableSidebar'
import { DataGrid } from '../components/db/DataGrid'
import { Pagination } from '../components/db/Pagination'
import { AddRowModal } from '../components/db/AddRowModal'
import { DeleteConfirmModal } from '../components/db/DeleteConfirmModal'

interface AppWithTables {
  app: EnvApp
  tables: DbTable[]
}

interface DbExplorerProps {
  currentEnv?: string
}

export function DbExplorer({ currentEnv }: DbExplorerProps) {
  const { slug: routeEnvSlug, appSlug: routeAppSlug } = useParams<{ slug: string; appSlug?: string }>()
  const [searchParams, setSearchParams] = useSearchParams()

  // Determine environment slug: route param > currentEnv
  const envSlug = routeEnvSlug || currentEnv || 'dev'

  // Current selection — from URL params or route
  const selectedAppSlug = searchParams.get('app') || routeAppSlug || null
  const selectedTable = searchParams.get('table') || null

  // Data state
  const [_environment, setEnvironment] = useState<Environment | null>(null)
  const [appsWithTables, setAppsWithTables] = useState<AppWithTables[]>([])
  const [schema, setSchema] = useState<DbSchema | null>(null)
  const [result, setResult] = useState<DbQueryResult | null>(null)

  // UI state
  const [sidebarLoading, setSidebarLoading] = useState(true)
  const [tableLoading, setTableLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Pagination
  const [pageSize, setPageSize] = useState(50)
  const [currentPage, setCurrentPage] = useState(0)

  // Sorting
  const [sortColumn, setSortColumn] = useState<string | null>(null)
  const [sortDesc, setSortDesc] = useState(false)

  // Filtering
  const [filters, setFilters] = useState<DbFilter[]>([])
  const [searchQuery, setSearchQuery] = useState('')
  const searchTimeout = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Selection
  const [selectedRows, setSelectedRows] = useState<Set<number>>(new Set())

  // Inline editing
  const [editingCell, setEditingCell] = useState<{ row: number; col: string } | null>(null)
  const [editValue, setEditValue] = useState('')
  const [savingCell, setSavingCell] = useState<{ row: number; col: string } | null>(null)

  // Modals
  const [showAddRow, setShowAddRow] = useState(false)
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false)

  // ── Load sidebar data ──────────────────────────────────────────
  useEffect(() => {
    if (!envSlug) return
    setSidebarLoading(true)
    setError(null)

    fetchEnvironments()
      .then(async (envs) => {
        const env = envs.find((e) => e.slug === envSlug)
        setEnvironment(env || null)
        const dbApps = (env?.apps || []).filter((a) => a.has_db)

        // Fetch tables for each DB app in parallel
        const results = await Promise.all(
          dbApps.map(async (app) => {
            try {
              const tables = await fetchDbTables(envSlug, app.slug)
              return { app, tables }
            } catch {
              return { app, tables: [] }
            }
          }),
        )
        setAppsWithTables(results)

        // Auto-select first table if coming from route with appSlug but no table
        if (routeAppSlug && !selectedTable) {
          const appData = results.find((r) => r.app.slug === routeAppSlug)
          if (appData && appData.tables.length > 0) {
            setSearchParams({ app: routeAppSlug, table: appData.tables[0].name }, { replace: true })
          }
        }
      })
      .catch((e) => setError(e.message))
      .finally(() => setSidebarLoading(false))
  }, [envSlug]) // eslint-disable-line react-hooks/exhaustive-deps

  // ── Load table data ────────────────────────────────────────────
  const loadTableData = useCallback(async () => {
    if (!envSlug || !selectedTable || !selectedAppSlug) {
      setSchema(null)
      setResult(null)
      return
    }

    setTableLoading(true)
    setError(null)

    try {
      // Build filters including search
      const allFilters = [...filters]

      const [schemaData, queryData] = await Promise.all([
        fetchDbSchema(envSlug, selectedTable, selectedAppSlug),
        queryDbData(envSlug, selectedTable, {
          limit: pageSize,
          offset: currentPage * pageSize,
          order_by: sortColumn || undefined,
          order_desc: sortDesc,
          filters: allFilters.length > 0 ? allFilters : undefined,
          app_slug: selectedAppSlug,
        }),
      ])
      setSchema(schemaData)
      setResult(queryData)
    } catch (e: any) {
      setError(e.message)
    } finally {
      setTableLoading(false)
    }
  }, [envSlug, selectedTable, selectedAppSlug, pageSize, currentPage, sortColumn, sortDesc, filters])

  useEffect(() => {
    loadTableData()
  }, [loadTableData])

  // ── Search handler (debounced) ─────────────────────────────────
  function handleSearchChange(value: string) {
    setSearchQuery(value)
    if (searchTimeout.current) clearTimeout(searchTimeout.current)
    searchTimeout.current = setTimeout(() => {
      if (value.trim()) {
        // Find a text column to search on — use first non-PK string column
        const textCol = schema?.columns.find(
          (c) => !c.primary_key && isTextType(c.data_type),
        )
        if (textCol) {
          setFilters((prev) => {
            const withoutSearch = prev.filter((f) => f.op !== 'like' || !f.value?.startsWith?.('%'))
            return [...withoutSearch, { column: textCol.name, op: 'like', value: `%${value}%` }]
          })
        }
      } else {
        setFilters((prev) => prev.filter((f) => f.op !== 'like' || !f.value?.startsWith?.('%')))
      }
      setCurrentPage(0)
    }, 400)
  }

  // ── Sorting ────────────────────────────────────────────────────
  function handleSort(column: string) {
    if (sortColumn === column) {
      if (sortDesc) {
        // third click: clear sort
        setSortColumn(null)
        setSortDesc(false)
      } else {
        setSortDesc(true)
      }
    } else {
      setSortColumn(column)
      setSortDesc(false)
    }
    setCurrentPage(0)
  }

  // ── Filtering ──────────────────────────────────────────────────
  function handleFilterChange(column: string, filter: DbFilter | null) {
    setFilters((prev) => {
      const withoutCol = prev.filter((f) => f.column !== column)
      return filter ? [...withoutCol, filter] : withoutCol
    })
    setCurrentPage(0)
  }

  // ── Selection ──────────────────────────────────────────────────
  function handleSelectRow(rowIndex: number, checked: boolean) {
    setSelectedRows((prev) => {
      const next = new Set(prev)
      if (checked) next.add(rowIndex)
      else next.delete(rowIndex)
      return next
    })
  }

  function handleSelectAll(checked: boolean) {
    if (checked && result) {
      setSelectedRows(new Set(result.rows.map((_, i) => i)))
    } else {
      setSelectedRows(new Set())
    }
  }

  // ── Inline editing ─────────────────────────────────────────────
  function handleStartEdit(row: number, col: string, value: any) {
    // Don't edit PKs
    const colSchema = schema?.columns.find((c) => c.name === col)
    if (colSchema?.primary_key) return
    setEditingCell({ row, col })
    setEditValue(value === null ? '' : String(value))
  }

  async function handleCommitEdit() {
    if (!editingCell || !result || !selectedAppSlug || !envSlug || !selectedTable) return

    const row = result.rows[editingCell.row]
    const originalValue = row[editingCell.col]
    const newValue = editValue

    // No change?
    if (String(originalValue ?? '') === newValue) {
      setEditingCell(null)
      return
    }

    // Find PK column and value for the WHERE clause
    const pkCol = schema?.columns.find((c) => c.primary_key)
    if (!pkCol) {
      setEditingCell(null)
      return
    }

    const pkValue = row[pkCol.name]
    setSavingCell(editingCell)
    setEditingCell(null)

    try {
      const updates: Record<string, any> = {
        [editingCell.col]: newValue === '' ? null : coerceForUpdate(newValue, editingCell.col),
      }
      await updateDbRows(envSlug, selectedAppSlug, selectedTable, updates, [
        { column: pkCol.name, op: 'eq', value: pkValue },
      ])
      await loadTableData()
    } catch (e: any) {
      setError(e.message)
    } finally {
      setSavingCell(null)
    }
  }

  function coerceForUpdate(value: string, colName: string): any {
    const col = schema?.columns.find((c) => c.name === colName)
    if (!col) return value
    const dt = col.data_type.toLowerCase()
    if (['integer', 'int', 'bigint', 'smallint'].includes(dt)) {
      const n = parseInt(value, 10)
      return isNaN(n) ? value : n
    }
    if (['real', 'float', 'double', 'numeric', 'decimal'].includes(dt)) {
      const n = parseFloat(value)
      return isNaN(n) ? value : n
    }
    if (dt === 'boolean' || dt === 'bool') {
      return value === 'true' || value === '1'
    }
    return value
  }

  // ── Add row ────────────────────────────────────────────────────
  async function handleInsertRow(row: Record<string, any>) {
    if (!envSlug || !selectedAppSlug || !selectedTable) return
    await insertDbRows(envSlug, selectedAppSlug, selectedTable, [row])
    await loadTableData()
  }

  // ── Delete rows ────────────────────────────────────────────────
  async function handleDeleteSelected() {
    if (!envSlug || !selectedAppSlug || !selectedTable || !result || !schema) return

    const pkCol = schema.columns.find((c) => c.primary_key)
    if (!pkCol) throw new Error('No primary key found')

    const pkValues = Array.from(selectedRows).map((idx) => result.rows[idx][pkCol.name])

    for (const pkVal of pkValues) {
      await deleteDbRows(envSlug, selectedAppSlug, selectedTable, [
        { column: pkCol.name, op: 'eq', value: pkVal },
      ])
    }

    setSelectedRows(new Set())
    await loadTableData()
  }

  // ── CSV export ─────────────────────────────────────────────────
  function handleExportCSV() {
    if (!result || result.rows.length === 0) return

    const headers = result.columns.join(',')
    const rows = result.rows.map((row) =>
      result.columns
        .map((col) => {
          const val = row[col]
          if (val === null) return ''
          const str = String(val)
          if (str.includes(',') || str.includes('"') || str.includes('\n')) {
            return `"${str.replace(/"/g, '""')}"`
          }
          return str
        })
        .join(','),
    )
    const csv = [headers, ...rows].join('\n')
    const blob = new Blob([csv], { type: 'text/csv' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `${selectedTable || 'export'}.csv`
    a.click()
    URL.revokeObjectURL(url)
  }

  // ── Table selection handler ────────────────────────────────────
  function handleSelectTable(appSlug: string, tableName: string) {
    setSearchParams({ app: appSlug, table: tableName })
    setCurrentPage(0)
    setSortColumn(null)
    setSortDesc(false)
    setFilters([])
    setSearchQuery('')
    setSelectedRows(new Set())
    setEditingCell(null)
  }

  // ── Pagination handlers ────────────────────────────────────────
  function handlePageChange(page: number) {
    setCurrentPage(page)
    setSelectedRows(new Set())
    setEditingCell(null)
  }

  function handlePageSizeChange(size: number) {
    setPageSize(size)
    setCurrentPage(0)
    setSelectedRows(new Set())
  }

  // Is this the standalone /tables route or env-scoped?
  const isStandaloneRoute = !routeEnvSlug

  return (
    <div className="flex flex-col h-full -m-6">
      {/* Header */}
      <div className="px-6 py-4 border-b border-white/5 shrink-0" style={{ background: '#0f0f23' }}>
        <div className="flex items-center gap-2 mb-1">
          {!isStandaloneRoute && (
            <>
              <Link to="/environments" className="text-xs text-white/25 hover:text-white/50 transition-colors">
                Environments
              </Link>
              <span className="text-xs text-white/10">/</span>
              <Link
                to={`/environments/${envSlug}`}
                className="text-xs text-white/25 hover:text-white/50 transition-colors"
              >
                {envSlug}
              </Link>
              <span className="text-xs text-white/10">/</span>
            </>
          )}
          <span className="text-xs text-white/40">Tables</span>
        </div>
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-bold text-[#e2e8f0]">
              {selectedTable ? (
                <>
                  <span className="text-white/30 font-normal">
                    {selectedAppSlug ? `${selectedAppSlug}.` : ''}
                  </span>
                  {selectedTable}
                </>
              ) : (
                'Data Browser'
              )}
            </h1>
            {result && selectedTable && (
              <p className="text-xs text-white/30 mt-0.5">
                {result.total_count.toLocaleString()} row{result.total_count !== 1 ? 's' : ''}
                {filters.length > 0 && (
                  <span className="text-[#a78bfa] ml-2">
                    {filters.length} filter{filters.length !== 1 ? 's' : ''} active
                  </span>
                )}
              </p>
            )}
          </div>

          {/* Action buttons */}
          {selectedTable && selectedAppSlug && (
            <div className="flex items-center gap-2">
              {/* Search bar */}
              <div className="relative">
                <svg
                  className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-white/20"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  strokeWidth={2}
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z"
                  />
                </svg>
                <input
                  type="text"
                  value={searchQuery}
                  onChange={(e) => handleSearchChange(e.target.value)}
                  placeholder="Search..."
                  className="w-48 text-xs rounded-md border border-white/10 bg-white/5 text-white/70 pl-8 pr-3 py-1.5 placeholder-white/15 focus:outline-none focus:border-[#7c3aed]/50 transition-colors"
                />
              </div>

              {/* Delete selected */}
              {selectedRows.size > 0 && (
                <button
                  onClick={() => setShowDeleteConfirm(true)}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md bg-red-600/15 text-red-400 hover:bg-red-600/25 border border-red-500/20 transition-colors"
                >
                  <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
                  </svg>
                  Delete ({selectedRows.size})
                </button>
              )}

              {/* Add row */}
              <button
                onClick={() => setShowAddRow(true)}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md bg-[#7c3aed] text-white hover:bg-[#6d28d9] transition-colors"
              >
                <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
                </svg>
                Add row
              </button>

              {/* Refresh */}
              <button
                onClick={loadTableData}
                disabled={tableLoading}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md bg-white/5 text-white/50 hover:bg-white/10 border border-white/10 transition-colors disabled:opacity-50"
              >
                <svg
                  className={`w-3.5 h-3.5 ${tableLoading ? 'animate-spin' : ''}`}
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  strokeWidth={2}
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182"
                  />
                </svg>
                Refresh
              </button>

              {/* Export CSV */}
              <button
                onClick={handleExportCSV}
                disabled={!result || result.rows.length === 0}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md bg-white/5 text-white/50 hover:bg-white/10 border border-white/10 transition-colors disabled:opacity-30"
              >
                <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />
                </svg>
                CSV
              </button>
            </div>
          )}
        </div>

        {/* Active filters bar */}
        {filters.length > 0 && (
          <div className="flex items-center gap-2 mt-3 flex-wrap">
            <span className="text-[10px] text-white/20 uppercase tracking-wider">Filters:</span>
            {filters.map((f, i) => (
              <span
                key={i}
                className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md text-xs bg-[#7c3aed]/10 text-[#a78bfa] border border-[#7c3aed]/20"
              >
                <span className="font-mono">{f.column}</span>
                <span className="text-white/25">{f.op}</span>
                {f.value != null && <span className="text-white/50">{String(f.value)}</span>}
                <button
                  onClick={() => handleFilterChange(f.column, null)}
                  className="text-white/30 hover:text-white/60 ml-0.5"
                >
                  <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </span>
            ))}
            <button
              onClick={() => {
                setFilters([])
                setSearchQuery('')
                setCurrentPage(0)
              }}
              className="text-xs text-white/25 hover:text-white/50 transition-colors"
            >
              Clear all
            </button>
          </div>
        )}
      </div>

      {/* Error banner */}
      {error && (
        <div className="mx-6 mt-3 bg-red-500/10 border border-red-500/30 rounded-lg px-4 py-2">
          <p className="text-sm text-red-400">{error}</p>
        </div>
      )}

      {/* Main content area */}
      <div className="flex flex-1 overflow-hidden">
        {/* Sidebar */}
        <div className="shrink-0 p-3 overflow-y-auto">
          <TableSidebar
            appsWithTables={appsWithTables}
            loading={sidebarLoading}
            selectedAppSlug={selectedAppSlug}
            selectedTable={selectedTable}
            onSelectTable={handleSelectTable}
          />
        </div>

        {/* Grid area */}
        <div className="flex-1 flex flex-col overflow-hidden p-3 pl-0">
          {!selectedTable || !selectedAppSlug ? (
            <div
              className="flex-1 flex items-center justify-center rounded-lg border border-white/5"
              style={{ background: '#1e1e3a' }}
            >
              <div className="text-center">
                <svg
                  className="w-12 h-12 text-white/10 mx-auto mb-3"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  strokeWidth={1}
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    d="M20.25 6.375c0 2.278-3.694 4.125-8.25 4.125S3.75 8.653 3.75 6.375m16.5 0c0-2.278-3.694-4.125-8.25-4.125S3.75 4.097 3.75 6.375m16.5 0v11.25c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125V6.375m16.5 0v3.75m-16.5-3.75v3.75m16.5 0v3.75C20.25 16.153 16.556 18 12 18s-8.25-1.847-8.25-4.125v-3.75m16.5 0c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125"
                  />
                </svg>
                <p className="text-white/25 text-sm">Select a table to browse data</p>
              </div>
            </div>
          ) : tableLoading && !result ? (
            <div className="flex-1 flex items-center justify-center">
              <div className="text-center">
                <div className="w-6 h-6 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin mx-auto mb-3" />
                <p className="text-white/25 text-sm">Loading...</p>
              </div>
            </div>
          ) : result ? (
            <div className="flex flex-col flex-1 overflow-hidden rounded-lg border border-white/5">
              {/* Data grid */}
              <div className={`flex-1 overflow-auto ${tableLoading ? 'opacity-60' : ''}`}>
                <DataGrid
                  result={result}
                  schema={schema?.columns || []}
                  filters={filters}
                  sortColumn={sortColumn}
                  sortDesc={sortDesc}
                  selectedRows={selectedRows}
                  editingCell={editingCell}
                  editValue={editValue}
                  savingCell={savingCell}
                  onSort={handleSort}
                  onFilterChange={handleFilterChange}
                  onSelectRow={handleSelectRow}
                  onSelectAll={handleSelectAll}
                  onStartEdit={handleStartEdit}
                  onEditChange={setEditValue}
                  onCommitEdit={handleCommitEdit}
                  onCancelEdit={() => setEditingCell(null)}
                />
              </div>

              {/* Pagination */}
              <Pagination
                totalCount={result.total_count}
                pageSize={pageSize}
                currentPage={currentPage}
                onPageChange={handlePageChange}
                onPageSizeChange={handlePageSizeChange}
              />
            </div>
          ) : null}
        </div>
      </div>

      {/* Modals */}
      {showAddRow && schema && (
        <AddRowModal
          columns={schema.columns}
          onSubmit={handleInsertRow}
          onClose={() => setShowAddRow(false)}
        />
      )}

      {showDeleteConfirm && (
        <DeleteConfirmModal
          count={selectedRows.size}
          onConfirm={handleDeleteSelected}
          onClose={() => setShowDeleteConfirm(false)}
        />
      )}
    </div>
  )
}

function isTextType(dt: string): boolean {
  const lower = dt.toLowerCase()
  return lower === 'text' || lower === 'varchar' || lower === 'char' || lower.startsWith('varchar')
}
