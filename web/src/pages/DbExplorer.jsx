import { useEffect, useState, useCallback, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import {
  listApps,
  getAppDbTables,
  getAppDbTable,
  queryAppDb,
  executeAppDb,
  snapshotAppDb,
} from '../api/client';
import { TableSidebar } from '../components/db/TableSidebar';
import { DataGrid } from '../components/db/DataGrid';
import { Pagination } from '../components/db/Pagination';
import { AddRowModal } from '../components/db/AddRowModal';
import { DeleteConfirmModal } from '../components/db/DeleteConfirmModal';
import { Download, Plus, Trash2, RefreshCw, Database, Camera, Loader2 } from 'lucide-react';

function unwrap(res) {
  const d = res.data;
  return (d && typeof d === 'object' && 'data' in d) ? d.data : d;
}

export default function DbExplorer({ appSlug: propAppSlug, embedded }) {
  const [searchParams, setSearchParams] = useSearchParams();

  const selectedAppSlug = propAppSlug || searchParams.get('app') || null;
  const selectedTable = searchParams.get('table') || null;

  // Data
  const [appsWithTables, setAppsWithTables] = useState([]);
  const [schema, setSchema] = useState(null);
  const [result, setResult] = useState(null);

  // UI
  const [sidebarLoading, setSidebarLoading] = useState(true);
  const [tableLoading, setTableLoading] = useState(false);
  const [error, setError] = useState(null);

  // Pagination
  const [pageSize, setPageSize] = useState(50);
  const [currentPage, setCurrentPage] = useState(0);

  // Sort
  const [sortColumn, setSortColumn] = useState(null);
  const [sortDesc, setSortDesc] = useState(false);

  // Filters
  const [filters, setFilters] = useState([]);
  const [searchQuery, setSearchQuery] = useState('');
  const searchTimeout = useRef(null);

  // Selection
  const [selectedRows, setSelectedRows] = useState(new Set());

  // Inline editing
  const [editingCell, setEditingCell] = useState(null);
  const [editValue, setEditValue] = useState('');
  const [savingCell, setSavingCell] = useState(null);

  // Modals
  const [showAddRow, setShowAddRow] = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);

  // Toast
  const [toast, setToast] = useState(null);

  function showToast(msg, type = 'ok') {
    setToast({ msg, type });
    setTimeout(() => setToast(null), 3000);
  }

  // ── Load sidebar ──
  useEffect(() => {
    setSidebarLoading(true);
    setError(null);

    const loadApps = async () => {
      try {
        const res = await listApps();
        const apps = unwrap(res)?.apps || unwrap(res) || [];
        const dbApps = (Array.isArray(apps) ? apps : []).filter(a => a.has_db);

        const results = await Promise.all(
          dbApps.map(async (app) => {
            try {
              const tablesRes = await getAppDbTables(app.slug);
              const raw = unwrap(tablesRes);
              const tables = raw?.tables || (Array.isArray(raw) ? raw : []);
              return { app, tables };
            } catch {
              return { app, tables: [] };
            }
          })
        );
        setAppsWithTables(results);

        // If propAppSlug is set (embedded mode), auto-select first table
        if (propAppSlug && !selectedTable) {
          const appData = results.find(r => r.app.slug === propAppSlug);
          if (appData && appData.tables.length > 0) {
            const name = typeof appData.tables[0] === 'string' ? appData.tables[0] : appData.tables[0].name;
            setSearchParams({ app: propAppSlug, table: name }, { replace: true });
          }
        }
      } catch (e) {
        setError(e.message);
      } finally {
        setSidebarLoading(false);
      }
    };
    loadApps();
  }, [propAppSlug]); // eslint-disable-line

  // ── Load table data ──
  const loadTableData = useCallback(async () => {
    const appSlug = selectedAppSlug;
    if (!appSlug || !selectedTable) {
      setSchema(null);
      setResult(null);
      return;
    }

    setTableLoading(true);
    setError(null);

    try {
      let where = '';
      const allFilters = [...filters];
      if (allFilters.length > 0) {
        const clauses = allFilters.map(f => {
          if (f.op === 'is_null') return `"${f.column}" IS NULL`;
          if (f.op === 'not_null') return `"${f.column}" IS NOT NULL`;
          const sqlOp = { eq: '=', neq: '!=', gt: '>', gte: '>=', lt: '<', lte: '<=', like: 'LIKE' }[f.op] || '=';
          return `"${f.column}" ${sqlOp} '${(f.value || '').replace(/'/g, "''")}'`;
        });
        where = ' WHERE ' + clauses.join(' AND ');
      }

      const order = sortColumn ? ` ORDER BY "${sortColumn}" ${sortDesc ? 'DESC' : 'ASC'}` : '';
      const limit = ` LIMIT ${pageSize} OFFSET ${currentPage * pageSize}`;
      const sql = `SELECT * FROM "${selectedTable}"${where}${order}${limit}`;
      const countSql = `SELECT COUNT(*) as cnt FROM "${selectedTable}"${where}`;

      const [schemaRes, queryRes, countRes] = await Promise.all([
        getAppDbTable(appSlug, selectedTable),
        queryAppDb(appSlug, sql),
        queryAppDb(appSlug, countSql),
      ]);

      const schemaData = unwrap(schemaRes);
      const queryData = unwrap(queryRes);
      const countData = unwrap(countRes);

      setSchema(schemaData);
      setResult({
        columns: queryData?.columns || [],
        rows: queryData?.rows || [],
        total_count: countData?.rows?.[0]?.cnt || queryData?.total || 0,
      });
    } catch (e) {
      setError(e.message);
    } finally {
      setTableLoading(false);
    }
  }, [selectedAppSlug, selectedTable, pageSize, currentPage, sortColumn, sortDesc, filters]);

  useEffect(() => { loadTableData(); }, [loadTableData]);

  // ── Search (debounced) ──
  function handleSearchChange(value) {
    setSearchQuery(value);
    if (searchTimeout.current) clearTimeout(searchTimeout.current);
    searchTimeout.current = setTimeout(() => {
      if (value.trim() && schema?.columns) {
        const textCol = schema.columns.find(c => !c.primary_key && isTextType(c.data_type || c.field_type));
        if (textCol) {
          setFilters(prev => {
            const without = prev.filter(f => f.op !== 'like' || !f.value?.startsWith?.('%'));
            return [...without, { column: textCol.name, op: 'like', value: `%${value}%` }];
          });
        }
      } else {
        setFilters(prev => prev.filter(f => f.op !== 'like' || !f.value?.startsWith?.('%')));
      }
      setCurrentPage(0);
    }, 400);
  }

  // ── Sort ──
  function handleSort(column) {
    if (sortColumn === column) {
      if (sortDesc) { setSortColumn(null); setSortDesc(false); }
      else setSortDesc(true);
    } else {
      setSortColumn(column);
      setSortDesc(false);
    }
    setCurrentPage(0);
  }

  // ── Filter ──
  function handleFilterChange(column, filter) {
    setFilters(prev => {
      const without = prev.filter(f => f.column !== column);
      return filter ? [...without, filter] : without;
    });
    setCurrentPage(0);
  }

  // ── Selection ──
  function handleSelectRow(idx, checked) {
    setSelectedRows(prev => { const n = new Set(prev); checked ? n.add(idx) : n.delete(idx); return n; });
  }
  function handleSelectAll(checked) {
    setSelectedRows(checked && result ? new Set(result.rows.map((_, i) => i)) : new Set());
  }

  // ── Inline edit ──
  function handleStartEdit(row, col, value) {
    const colSchema = schema?.columns?.find(c => c.name === col);
    if (colSchema?.primary_key) return;
    setEditingCell({ row, col });
    setEditValue(value == null ? '' : String(value));
  }

  async function handleCommitEdit() {
    if (!editingCell || !result || !selectedAppSlug || !selectedTable) return;

    const row = result.rows[editingCell.row];
    const original = row[editingCell.col];
    if (String(original ?? '') === editValue) { setEditingCell(null); return; }

    const pkCol = schema?.columns?.find(c => c.primary_key);
    if (!pkCol) { setEditingCell(null); return; }

    const pkValue = row[pkCol.name];
    setSavingCell(editingCell);
    setEditingCell(null);

    try {
      const newVal = editValue === '' ? 'NULL' : `'${editValue.replace(/'/g, "''")}'`;
      const sql = `UPDATE "${selectedTable}" SET "${editingCell.col}" = ${newVal} WHERE "${pkCol.name}" = '${String(pkValue).replace(/'/g, "''")}'`;
      await executeAppDb(selectedAppSlug, sql);
      await loadTableData();
      showToast('Cellule mise a jour');
    } catch (e) {
      setError(e.message);
    } finally {
      setSavingCell(null);
    }
  }

  // ── Insert row ──
  async function handleInsertRow(rowData) {
    if (!selectedAppSlug || !selectedTable) return;
    const cols = Object.keys(rowData);
    const vals = cols.map(c => {
      const v = rowData[c];
      return v == null ? 'NULL' : `'${String(v).replace(/'/g, "''")}'`;
    });
    const sql = `INSERT INTO "${selectedTable}" (${cols.map(c => `"${c}"`).join(', ')}) VALUES (${vals.join(', ')})`;
    await executeAppDb(selectedAppSlug, sql);
    await loadTableData();
    showToast('Ligne ajoutee');
  }

  // ── Delete rows ──
  async function handleDeleteSelected() {
    if (!selectedAppSlug || !selectedTable || !result || !schema) return;
    const pkCol = schema.columns?.find(c => c.primary_key);
    if (!pkCol) throw new Error('No PK');

    const pkValues = Array.from(selectedRows).map(idx => result.rows[idx][pkCol.name]);
    for (const pk of pkValues) {
      const sql = `DELETE FROM "${selectedTable}" WHERE "${pkCol.name}" = '${String(pk).replace(/'/g, "''")}'`;
      await executeAppDb(selectedAppSlug, sql);
    }
    setSelectedRows(new Set());
    await loadTableData();
    showToast(`${pkValues.length} ligne(s) supprimee(s)`);
  }

  // ── Export CSV ──
  function handleExportCSV() {
    if (!result || result.rows.length === 0) return;
    const headers = result.columns.join(',');
    const rows = result.rows.map(row =>
      result.columns.map(col => {
        const val = row[col];
        if (val == null) return '';
        const str = String(val);
        return str.includes(',') || str.includes('"') || str.includes('\n') ? `"${str.replace(/"/g, '""')}"` : str;
      }).join(',')
    );
    const csv = [headers, ...rows].join('\n');
    const blob = new Blob([csv], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${selectedTable || 'export'}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  }

  // ── Snapshot ──
  async function handleSnapshot() {
    if (!selectedAppSlug) return;
    try {
      const res = await snapshotAppDb(selectedAppSlug);
      const data = unwrap(res);
      showToast(`Snapshot : ${data?.path || 'OK'}`);
    } catch (e) {
      setError(e.message);
    }
  }

  // ── Select table ──
  function handleSelectTable(appSlug, tableName) {
    setSearchParams({ app: appSlug, table: tableName });
    setCurrentPage(0);
    setSortColumn(null);
    setSortDesc(false);
    setFilters([]);
    setSearchQuery('');
    setSelectedRows(new Set());
    setEditingCell(null);
  }

  const totalCount = result?.total_count || 0;

  return (
    <div className={`flex h-full overflow-hidden ${embedded ? '' : 'rounded border border-gray-700'}`}>
      {/* Sidebar */}
      {!propAppSlug && (
        <TableSidebar
          appsWithTables={appsWithTables}
          selectedAppSlug={selectedAppSlug}
          selectedTable={selectedTable}
          onSelectTable={handleSelectTable}
          loading={sidebarLoading}
        />
      )}

      {/* Main */}
      <div className="flex flex-col flex-1 min-w-0">
        {/* Toolbar */}
        <div className="flex items-center gap-2 px-4 py-2 border-b border-gray-700 shrink-0 bg-gray-800/50">
          <div className="flex items-center gap-2 flex-1">
            {selectedTable ? (
              <>
                <Database className="w-4 h-4 text-blue-400" />
                <span className="text-sm font-medium text-white">
                  {selectedAppSlug && <span className="text-gray-500">{selectedAppSlug}.</span>}
                  {selectedTable}
                </span>
                {totalCount > 0 && <span className="text-xs text-gray-500">({totalCount.toLocaleString()} lignes)</span>}
              </>
            ) : (
              <span className="text-sm text-gray-500">Selectionnez une table</span>
            )}
          </div>

          {selectedTable && (
            <div className="flex items-center gap-1">
              <input
                type="text"
                value={searchQuery}
                onChange={e => handleSearchChange(e.target.value)}
                placeholder="Rechercher..."
                className="bg-gray-900 text-white text-xs rounded px-2 py-1 border border-gray-600 w-40 outline-none"
              />
              <button onClick={() => setShowAddRow(true)} className="p-1.5 text-gray-400 hover:text-green-400 hover:bg-gray-700 rounded border-none bg-transparent cursor-pointer" title="Ajouter">
                <Plus className="w-3.5 h-3.5" />
              </button>
              {selectedRows.size > 0 && (
                <button onClick={() => setShowDeleteConfirm(true)} className="p-1.5 text-gray-400 hover:text-red-400 hover:bg-gray-700 rounded border-none bg-transparent cursor-pointer" title="Supprimer">
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              )}
              <button onClick={handleExportCSV} disabled={!result?.rows?.length} className="p-1.5 text-gray-400 hover:text-white hover:bg-gray-700 rounded border-none bg-transparent cursor-pointer disabled:opacity-30" title="Exporter CSV">
                <Download className="w-3.5 h-3.5" />
              </button>
              <button onClick={handleSnapshot} className="p-1.5 text-gray-400 hover:text-white hover:bg-gray-700 rounded border-none bg-transparent cursor-pointer" title="Snapshot">
                <Camera className="w-3.5 h-3.5" />
              </button>
              <button onClick={loadTableData} className="p-1.5 text-gray-400 hover:text-white hover:bg-gray-700 rounded border-none bg-transparent cursor-pointer" title="Actualiser">
                <RefreshCw className="w-3.5 h-3.5" />
              </button>
            </div>
          )}
        </div>

        {/* Error */}
        {error && (
          <div className="px-4 py-2 text-xs bg-red-500/10 text-red-400 border-b border-red-500/20 shrink-0">
            {error}
          </div>
        )}

        {/* Grid */}
        <div className="flex-1 overflow-hidden">
          {selectedTable ? (
            <DataGrid
              columns={result?.columns || []}
              rows={result?.rows || []}
              schema={schema}
              sortColumn={sortColumn}
              sortDesc={sortDesc}
              onSort={handleSort}
              filters={filters}
              onFilterChange={handleFilterChange}
              selectedRows={selectedRows}
              onSelectRow={handleSelectRow}
              onSelectAll={handleSelectAll}
              editingCell={editingCell}
              editValue={editValue}
              savingCell={savingCell}
              onStartEdit={handleStartEdit}
              onEditValueChange={setEditValue}
              onCommitEdit={handleCommitEdit}
              onCancelEdit={() => setEditingCell(null)}
              loading={tableLoading}
            />
          ) : (
            <div className="flex flex-col items-center justify-center h-full text-gray-500">
              <Database className="w-12 h-12 mb-3 opacity-20" />
              <p className="text-sm">Selectionnez une table{!propAppSlug ? ' dans la barre laterale' : ''}</p>
            </div>
          )}
        </div>

        {/* Pagination */}
        {selectedTable && totalCount > 0 && (
          <Pagination
            currentPage={currentPage}
            pageSize={pageSize}
            totalCount={totalCount}
            onPageChange={(p) => { setCurrentPage(p); setSelectedRows(new Set()); setEditingCell(null); }}
            onPageSizeChange={(s) => { setPageSize(s); setCurrentPage(0); setSelectedRows(new Set()); }}
          />
        )}
      </div>

      {/* Toast */}
      {toast && (
        <div className={`fixed bottom-4 right-4 z-50 px-4 py-2 rounded-lg text-sm shadow-lg ${
          toast.type === 'ok' ? 'bg-green-500/90 text-white' : 'bg-red-500/90 text-white'
        }`}>
          {toast.msg}
        </div>
      )}

      {/* Modals */}
      {showAddRow && schema && (
        <AddRowModal columns={schema.columns || []} onInsert={handleInsertRow} onClose={() => setShowAddRow(false)} />
      )}
      {showDeleteConfirm && (
        <DeleteConfirmModal count={selectedRows.size} onConfirm={handleDeleteSelected} onClose={() => setShowDeleteConfirm(false)} />
      )}
    </div>
  );
}

function isTextType(type) {
  if (!type) return false;
  const t = type.toLowerCase();
  return ['text', 'varchar', 'char', 'string', 'clob', 'nvarchar', 'nchar'].some(k => t.includes(k));
}
