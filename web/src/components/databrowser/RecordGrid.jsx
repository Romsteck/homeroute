import { useState, useRef, useEffect } from 'react';
import {
  ChevronUp, ChevronDown, ListFilter, Loader2, Table2
} from 'lucide-react';
import LookupLink from './LookupLink';
import ColumnFilter from './ColumnFilter';
import ActiveFilters from './ActiveFilters';
import Pagination from './Pagination';

const SYSTEM_COLUMNS = ['id', 'created_at', 'updated_at'];

const OP_LABELS = {
  eq: '=', ne: '≠', gt: '>', lt: '<', gte: '≥', lte: '≤',
  like: 'contient', is_null: 'est vide', is_not_null: 'non vide',
};

function formatCellValue(value, fieldType) {
  if (value === null || value === undefined) return <span className="text-gray-600 italic">null</span>;

  switch (fieldType) {
    case 'boolean':
      return value ? (
        <span className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium bg-green-900/40 text-green-400">Oui</span>
      ) : (
        <span className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium bg-gray-700 text-gray-400">Non</span>
      );
    case 'currency': {
      const num = Number(value);
      return isNaN(num) ? String(value) : new Intl.NumberFormat('fr-FR', { style: 'currency', currency: 'USD' }).format(num);
    }
    case 'percent': {
      const num = Number(value);
      return isNaN(num) ? String(value) : `${num}%`;
    }
    case 'number': {
      const num = Number(value);
      return isNaN(num) ? String(value) : new Intl.NumberFormat('fr-FR').format(num);
    }
    case 'decimal': {
      const num = Number(value);
      return isNaN(num) ? String(value) : new Intl.NumberFormat('fr-FR', { minimumFractionDigits: 2 }).format(num);
    }
    case 'date':
      return formatDate(value);
    case 'date_time':
      return formatDateTime(value);
    case 'time':
      return String(value).slice(0, 5);
    case 'choice':
      return <ChoiceBadge value={value} />;
    case 'json': {
      const str = typeof value === 'object' ? JSON.stringify(value) : String(value);
      return <span className="font-mono text-xs text-gray-400">{str.length > 60 ? str.slice(0, 60) + '...' : str}</span>;
    }
    default: {
      const str = String(value);
      return str.length > 120 ? str.slice(0, 120) + '...' : str;
    }
  }
}

function formatDate(v) {
  if (!v) return '';
  try {
    const d = new Date(v);
    return d.toLocaleDateString('fr-FR');
  } catch { return String(v); }
}

function formatDateTime(v) {
  if (!v) return '';
  try {
    const d = new Date(v);
    return d.toLocaleDateString('fr-FR') + ' ' + d.toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
  } catch { return String(v); }
}

function choiceColor(str) {
  const hue = str.split('').reduce((a, c) => a + c.charCodeAt(0), 0) % 360;
  return `hsl(${hue}, 60%, 50%)`;
}

function ChoiceBadge({ value }) {
  if (!value) return null;
  return (
    <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium bg-gray-700">
      <span className="w-2 h-2 rounded-full flex-shrink-0" style={{ backgroundColor: choiceColor(String(value)) }} />
      {value}
    </span>
  );
}

export default function RecordGrid({
  columns,
  rows,
  total,
  loading,
  page,
  rowsPerPage,
  orderBy,
  orderDesc,
  tableName,
  onSort,
  onPageChange,
  onRowClick,
  onLookupClick,
  lookupResolver,
  filters,
  onFilterApply,
  onFilterRemove,
  onFiltersClearAll,
}) {
  const [openFilter, setOpenFilter] = useState(null);
  const filterRef = useRef(null);
  const totalPages = Math.max(1, Math.ceil(total / rowsPerPage));

  const allColumns = ['id', ...columns.filter(c => !SYSTEM_COLUMNS.includes(c.name)).map(c => c.name), 'created_at', 'updated_at'];

  // Close filter dropdown on outside click
  useEffect(() => {
    if (!openFilter) return;
    const handler = (e) => {
      if (filterRef.current && !filterRef.current.contains(e.target)) {
        setOpenFilter(null);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [openFilter]);

  function getColumnInfo(name) {
    return columns.find(c => c.name === name);
  }

  function getColumnFieldType(name) {
    if (name === 'id') return 'auto_increment';
    if (name === 'created_at' || name === 'updated_at') return 'date_time';
    const col = getColumnInfo(name);
    return col?.field_type || 'text';
  }

  function handleFilterApply(filter) {
    onFilterApply(filter);
    setOpenFilter(null);
  }

  function handleFilterClear(colName) {
    const idx = filters.findIndex(f => f.column === colName);
    if (idx !== -1) onFilterRemove(idx);
    setOpenFilter(null);
  }

  // Build display info for active filters
  const displayFilters = filters.map(f => ({
    ...f,
    displayOp: OP_LABELS[f.op] || f.op,
    displayValue: f.op === 'is_null' ? '' : f.op === 'like' ? String(f.value).replace(/%/g, '') : String(f.value),
  }));

  if (loading) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <Loader2 className="w-6 h-6 text-gray-500 animate-spin" />
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col min-h-0">
      {/* Active filters bar */}
      <ActiveFilters
        filters={displayFilters}
        onRemove={onFilterRemove}
        onClearAll={onFiltersClearAll}
      />

      {/* Data table */}
      <div className="flex-1 min-h-0 overflow-auto">
        {rows.length === 0 ? (
          <div className="flex items-center justify-center py-12">
            <div className="text-center">
              <Table2 className="w-10 h-10 text-gray-600 mx-auto mb-2" />
              <p className="text-gray-500 text-sm">
                {filters.length > 0 ? 'Aucun resultat pour ces filtres.' : 'Aucune donnee dans cette table.'}
              </p>
            </div>
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead className="sticky top-0 z-10">
              <tr className="bg-gray-800 border-b border-gray-700">
                {allColumns.map(col => {
                  const fieldType = getColumnFieldType(col);
                  const colInfo = getColumnInfo(col);
                  const isSystem = SYSTEM_COLUMNS.includes(col);
                  const hasFilter = filters.some(f => f.column === col);
                  const currentFilter = filters.find(f => f.column === col);

                  return (
                    <th
                      key={col}
                      className="px-3 py-2 text-left text-xs font-semibold text-gray-400 uppercase tracking-wider select-none whitespace-nowrap relative"
                    >
                      <div className="flex items-center gap-1">
                        <span
                          className="cursor-pointer hover:text-gray-200 transition-colors"
                          onClick={() => onSort(col)}
                        >
                          {col}
                        </span>
                        {orderBy === col && (
                          orderDesc
                            ? <ChevronDown className="w-3 h-3 text-blue-400" />
                            : <ChevronUp className="w-3 h-3 text-blue-400" />
                        )}
                        {!isSystem && (
                          <button
                            onClick={(e) => { e.stopPropagation(); setOpenFilter(openFilter === col ? null : col); }}
                            className={`ml-1 p-0.5 rounded transition-colors ${
                              hasFilter ? 'text-blue-400 bg-blue-900/30' : 'text-gray-600 hover:text-gray-400'
                            }`}
                            title={`Filtrer ${col}`}
                          >
                            <ListFilter className="w-3 h-3" />
                          </button>
                        )}
                      </div>

                      {/* Column filter dropdown */}
                      {openFilter === col && (
                        <div ref={filterRef} className="absolute top-full left-0 mt-1">
                          <ColumnFilter
                            column={col}
                            fieldType={fieldType}
                            choices={colInfo?.choices || []}
                            currentFilter={currentFilter ? { op: currentFilter.op, value: currentFilter.value } : null}
                            onApply={handleFilterApply}
                            onClear={() => handleFilterClear(col)}
                            onClose={() => setOpenFilter(null)}
                          />
                        </div>
                      )}
                    </th>
                  );
                })}
              </tr>
            </thead>
            <tbody>
              {rows.map((row, idx) => (
                <tr
                  key={row.id ?? idx}
                  onClick={() => onRowClick(row.id)}
                  className={`border-b border-gray-700/50 transition-colors cursor-pointer ${
                    idx % 2 === 0 ? 'bg-gray-900/40' : 'bg-gray-900/20'
                  } hover:bg-blue-900/20`}
                >
                  {allColumns.map(col => {
                    const fieldType = getColumnFieldType(col);

                    // Lookup columns render as links
                    if (fieldType === 'lookup') {
                      const relation = lookupResolver?.getRelation(tableName, col);
                      if (relation) {
                        return (
                          <td key={col} className="px-3 py-2 whitespace-nowrap">
                            <LookupLink
                              targetTable={relation.to_table}
                              value={row[col]}
                              onClick={(table, id) => onLookupClick(table, id)}
                            />
                          </td>
                        );
                      }
                    }

                    // ID column - show as clickable blue link
                    if (col === 'id') {
                      return (
                        <td key={col} className="px-3 py-2 whitespace-nowrap">
                          <span className="text-blue-400 font-medium">#{row[col]}</span>
                        </td>
                      );
                    }

                    return (
                      <td
                        key={col}
                        className="px-3 py-2 text-gray-300 whitespace-nowrap max-w-[300px] truncate"
                        title={row[col] != null ? String(row[col]) : ''}
                      >
                        {formatCellValue(row[col], fieldType)}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="bg-gray-800 border-t border-gray-700 px-4 py-2 flex-shrink-0">
          <Pagination page={page} totalPages={totalPages} onPageChange={onPageChange} />
        </div>
      )}
    </div>
  );
}
