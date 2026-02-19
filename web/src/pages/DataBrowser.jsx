import { useState, useEffect, useCallback, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import { Database, Table2, Loader2 } from 'lucide-react';
import PageHeader from '../components/PageHeader';
import SitemapPanel from '../components/databrowser/SitemapPanel';
import CommandBar from '../components/databrowser/CommandBar';
import RecordGrid from '../components/databrowser/RecordGrid';
import RecordForm from '../components/databrowser/RecordForm';
import AddRowModal from '../components/databrowser/AddRowModal';
import useLookupResolver from '../components/databrowser/useLookupResolver';
import {
  getDataverseOverview, getDataverseTable, getDataverseRows,
  insertDataverseRows, downloadDataverseBackup
} from '../api/client';

const ROWS_PER_PAGE_OPTIONS = [25, 50, 100];

function DataBrowser() {
  const [searchParams, setSearchParams] = useSearchParams();

  // URL state
  const appId = searchParams.get('app');
  const tableName = searchParams.get('table');
  const recordId = searchParams.get('id');
  const viewMode = recordId ? 'form' : tableName ? 'grid' : 'sitemap';

  // App data
  const [apps, setApps] = useState([]);
  const [loading, setLoading] = useState(true);
  const [envFilter, setEnvFilter] = useState(() => localStorage.getItem('databrowser-env') || 'production');

  // Table schema + data
  const [tableInfo, setTableInfo] = useState(null);
  const [rows, setRows] = useState([]);
  const [total, setTotal] = useState(0);
  const [loadingRows, setLoadingRows] = useState(false);

  // Pagination & sorting
  const [page, setPage] = useState(1);
  const [rowsPerPage, setRowsPerPage] = useState(50);
  const [orderBy, setOrderBy] = useState('id');
  const [orderDesc, setOrderDesc] = useState(true);

  // Filters
  const [filters, setFilters] = useState([]);

  // Modals
  const [showAddModal, setShowAddModal] = useState(false);

  // Lookup resolver
  const lookupResolver = useLookupResolver(appId);

  // Form ref for CommandBar → RecordForm communication
  const formRef = useRef(null);
  const [formState, setFormState] = useState({ isDirty: false, saving: false });

  // Poll form ref state so CommandBar stays in sync
  useEffect(() => {
    if (viewMode !== 'form') return;
    const interval = setInterval(() => {
      if (formRef.current) {
        setFormState({
          isDirty: !!formRef.current.isDirty,
          saving: !!formRef.current.saving,
        });
      }
    }, 100);
    return () => clearInterval(interval);
  }, [viewMode]);

  const columns = tableInfo?.columns || [];
  const selectedApp = apps.find(a => a.appId === appId);
  const filteredApps = apps.filter(a => a.environment === envFilter);

  // ── Navigation functions ─────────────────────────────

  function navigateToTable(newAppId, newTableName) {
    setSearchParams({ app: newAppId, table: newTableName });
    setPage(1);
    setOrderBy('id');
    setOrderDesc(true);
    setFilters([]);
  }

  function navigateToRecord(newAppId, newTableName, newRecordId) {
    setSearchParams({ app: newAppId, table: newTableName, id: String(newRecordId) });
  }

  function navigateHome() {
    setSearchParams({});
    setTableInfo(null);
    setRows([]);
    setTotal(0);
    setFilters([]);
  }

  function navigateToApp() {
    if (appId) setSearchParams({ app: appId });
    setTableInfo(null);
    setRows([]);
    setFilters([]);
  }

  function navigateToTableView() {
    if (appId && tableName) setSearchParams({ app: appId, table: tableName });
  }

  // ── Data fetching ────────────────────────────────────

  const fetchApps = useCallback(async () => {
    try {
      const res = await getDataverseOverview();
      setApps(res.data?.apps || []);
    } catch (err) {
      console.error('Failed to fetch apps:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchApps(); }, [fetchApps]);

  // Auto-select first app when none selected and apps loaded
  useEffect(() => {
    if (apps.length > 0 && !appId) {
      const filtered = apps.filter(a => a.environment === envFilter);
      if (filtered.length > 0) {
        setSearchParams({ app: filtered[0].appId });
      }
    }
  }, [apps, appId, envFilter]);

  // Persist env filter
  useEffect(() => {
    localStorage.setItem('databrowser-env', envFilter);
  }, [envFilter]);

  // Handle env filter change
  function handleEnvChange(newEnv) {
    setEnvFilter(newEnv);
    // If current app doesn't match new env, reset
    if (selectedApp && selectedApp.environment !== newEnv) {
      const filtered = apps.filter(a => a.environment === newEnv);
      if (filtered.length > 0) {
        setSearchParams({ app: filtered[0].appId });
      } else {
        setSearchParams({});
      }
      setTableInfo(null);
      setRows([]);
      setTotal(0);
      setFilters([]);
    }
  }

  // Fetch table schema when table changes
  useEffect(() => {
    if (!appId || !tableName) {
      setTableInfo(null);
      setRows([]);
      setTotal(0);
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const res = await getDataverseTable(appId, tableName);
        if (!cancelled) setTableInfo(res.data?.table || null);
      } catch {
        if (!cancelled) setTableInfo(null);
      }
    })();
    return () => { cancelled = true; };
  }, [appId, tableName]);

  // Fetch rows when table/page/sort/filters change
  const fetchRows = useCallback(async () => {
    if (!appId || !tableName || viewMode !== 'grid') return;
    setLoadingRows(true);
    try {
      const params = {
        limit: rowsPerPage,
        offset: (page - 1) * rowsPerPage,
        order_by: orderBy,
        order_desc: orderDesc,
      };
      if (filters.length > 0) {
        params.filters = JSON.stringify(filters);
      }
      const res = await getDataverseRows(appId, tableName, params);
      setRows(res.data?.data?.rows || []);
      setTotal(res.data?.data?.total || 0);
    } catch (err) {
      console.error('Failed to fetch rows:', err);
    } finally {
      setLoadingRows(false);
    }
  }, [appId, tableName, page, rowsPerPage, orderBy, orderDesc, filters, viewMode]);

  useEffect(() => { if (tableInfo && viewMode === 'grid') fetchRows(); }, [fetchRows, tableInfo, viewMode]);

  // ── Event handlers ───────────────────────────────────

  function handleSort(col) {
    if (orderBy === col) {
      setOrderDesc(!orderDesc);
    } else {
      setOrderBy(col);
      setOrderDesc(false);
    }
    setPage(1);
  }

  async function handleAddRow(values) {
    await insertDataverseRows(appId, tableName, [values]);
    fetchRows();
    fetchApps(); // refresh row counts
  }

  function handleFilterApply(filter) {
    setFilters(prev => {
      // Replace existing filter for same column, or add new
      const existing = prev.findIndex(f => f.column === filter.column);
      if (existing !== -1) {
        const updated = [...prev];
        updated[existing] = filter;
        return updated;
      }
      return [...prev, filter];
    });
    setPage(1);
  }

  function handleFilterRemove(index) {
    setFilters(prev => prev.filter((_, i) => i !== index));
    setPage(1);
  }

  function handleFiltersClearAll() {
    setFilters([]);
    setPage(1);
  }

  function handleRowClick(rowId) {
    navigateToRecord(appId, tableName, rowId);
  }

  function handleLookupClick(targetTable, targetId) {
    navigateToRecord(appId, targetTable, targetId);
  }

  function handleRowsPerPageChange(n) {
    setRowsPerPage(n);
    setPage(1);
  }

  // ── Render ───────────────────────────────────────────

  if (loading) {
    return (
      <div className="h-full flex flex-col">
        <PageHeader icon={Table2} title="Data Browser" />
        <div className="flex-1 flex items-center justify-center">
          <Loader2 className="w-8 h-8 text-blue-400 animate-spin" />
        </div>
      </div>
    );
  }

  if (apps.length === 0) {
    return (
      <div className="h-full flex flex-col">
        <PageHeader icon={Table2} title="Data Browser" />
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center">
            <Database className="w-12 h-12 text-gray-600 mx-auto mb-3" />
            <p className="text-gray-400">Aucune application avec Dataverse.</p>
            <p className="text-gray-500 text-sm mt-1">Les donnees apparaitront ici quand une app aura cree des tables.</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <PageHeader icon={Table2} title="Data Browser">
        <div className="flex items-center gap-1 bg-gray-700/50 rounded-lg p-0.5">
          <button
            onClick={() => handleEnvChange('production')}
            className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
              envFilter === 'production'
                ? 'bg-purple-600 text-white'
                : 'text-gray-400 hover:text-gray-200'
            }`}
          >
            PROD
          </button>
          <button
            onClick={() => handleEnvChange('development')}
            className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
              envFilter === 'development'
                ? 'bg-blue-600 text-white'
                : 'text-gray-400 hover:text-gray-200'
            }`}
          >
            DEV
          </button>
        </div>
      </PageHeader>

      <div className="flex-1 min-h-0 flex">
        {/* Left panel: Sitemap */}
        <SitemapPanel
          apps={filteredApps}
          selectedAppId={appId}
          selectedTableName={tableName}
          envFilter={envFilter}
          onSelectTable={navigateToTable}
        />

        {/* Main panel: CommandBar + content */}
        <div className="flex-1 flex flex-col min-w-0">
          <CommandBar
            viewMode={viewMode}
            appSlug={selectedApp?.slug}
            tableName={tableName}
            recordId={recordId}
            onNavigateHome={navigateHome}
            onNavigateApp={navigateToApp}
            onNavigateTable={navigateToTableView}
            onAddRow={() => setShowAddModal(true)}
            onRefresh={fetchRows}
            onBackup={() => downloadDataverseBackup(appId)}
            totalRows={total}
            rowsPerPage={rowsPerPage}
            rowsPerPageOptions={ROWS_PER_PAGE_OPTIONS}
            onRowsPerPageChange={handleRowsPerPageChange}
            onBack={navigateToTableView}
            onSave={() => formRef.current?.handleSave()}
            onDelete={() => formRef.current?.handleDelete()}
            isDirty={formState.isDirty}
            isSaving={formState.saving}
          />

          {viewMode === 'sitemap' && (
            <div className="flex-1 flex items-center justify-center">
              <div className="text-center">
                <Table2 className="w-10 h-10 text-gray-600 mx-auto mb-2" />
                <p className="text-gray-500 text-sm">Selectionnez une table pour voir ses donnees.</p>
              </div>
            </div>
          )}

          {viewMode === 'grid' && (
            <RecordGrid
              columns={columns}
              rows={rows}
              total={total}
              loading={loadingRows}
              page={page}
              rowsPerPage={rowsPerPage}
              orderBy={orderBy}
              orderDesc={orderDesc}
              tableName={tableName}
              onSort={handleSort}
              onPageChange={setPage}
              onRowClick={handleRowClick}
              onLookupClick={handleLookupClick}
              lookupResolver={lookupResolver}
              filters={filters}
              onFilterApply={handleFilterApply}
              onFilterRemove={handleFilterRemove}
              onFiltersClearAll={handleFiltersClearAll}
            />
          )}

          {viewMode === 'form' && (
            <RecordForm
              ref={formRef}
              appId={appId}
              tableName={tableName}
              recordId={recordId}
              columns={columns}
              lookupResolver={lookupResolver}
              onBack={navigateToTableView}
              onLookupNavigate={(table, id) => navigateToRecord(appId, table, id)}
              onDeleted={() => {
                navigateToTableView();
                fetchRows();
                fetchApps();
              }}
            />
          )}
        </div>
      </div>

      {/* Modals */}
      {showAddModal && (
        <AddRowModal
          columns={columns}
          lookupResolver={lookupResolver}
          tableName={tableName}
          onClose={() => setShowAddModal(false)}
          onAdd={handleAddRow}
        />
      )}
    </div>
  );
}

export default DataBrowser;
