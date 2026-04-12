import { ArrowUp, ArrowDown, Loader2 } from 'lucide-react';
import { FilterDropdown } from './FilterDropdown';

export function DataGrid({
  columns,
  rows,
  schema,
  sortColumn,
  sortDesc,
  onSort,
  filters,
  onFilterChange,
  selectedRows,
  onSelectRow,
  onSelectAll,
  editingCell,
  editValue,
  savingCell,
  onStartEdit,
  onEditValueChange,
  onCommitEdit,
  onCancelEdit,
  loading,
}) {
  if (loading) {
    return (
      <div className="flex items-center justify-center h-full text-gray-500 text-sm">
        <Loader2 className="w-5 h-5 animate-spin mr-2" /> Chargement...
      </div>
    );
  }

  if (!columns || columns.length === 0) {
    return <div className="flex items-center justify-center h-full text-gray-500 text-sm">Aucune donnee</div>;
  }

  const schemaMap = {};
  if (schema?.columns) {
    schema.columns.forEach(c => { schemaMap[c.name] = c; });
  }

  const allSelected = rows.length > 0 && selectedRows.size === rows.length;

  return (
    <div className="overflow-auto h-full">
      <table className="w-full text-sm border-collapse">
        <thead className="sticky top-0 z-10">
          <tr className="bg-gray-800">
            {/* Checkbox column */}
            <th className="w-10 px-3 py-2 border-b border-gray-700">
              <input
                type="checkbox"
                checked={allSelected}
                onChange={e => onSelectAll(e.target.checked)}
                className="cursor-pointer"
              />
            </th>
            {columns.map(col => {
              const currentFilter = filters.find(f => f.column === col);
              const isSorted = sortColumn === col;
              return (
                <th key={col} className="px-3 py-2 text-left text-xs font-semibold text-gray-400 border-b border-gray-700 whitespace-nowrap">
                  <div className="flex items-center gap-1">
                    <button
                      onClick={() => onSort(col)}
                      className="flex items-center gap-1 border-none bg-transparent text-gray-400 hover:text-white cursor-pointer text-xs font-semibold"
                    >
                      {col}
                      {isSorted && (sortDesc ? <ArrowDown className="w-3 h-3 text-blue-400" /> : <ArrowUp className="w-3 h-3 text-blue-400" />)}
                    </button>
                    {schemaMap[col]?.primary_key && <span className="text-[9px] text-yellow-400 font-bold">PK</span>}
                    <FilterDropdown column={col} currentFilter={currentFilter} onFilterChange={onFilterChange} />
                  </div>
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, rowIdx) => {
            const isSelected = selectedRows.has(rowIdx);
            return (
              <tr key={rowIdx} className={`${isSelected ? 'bg-blue-500/10' : rowIdx % 2 ? 'bg-gray-800/30' : ''} hover:bg-gray-700/30`}>
                <td className="px-3 py-1.5 border-b border-gray-700/50">
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={e => onSelectRow(rowIdx, e.target.checked)}
                    className="cursor-pointer"
                  />
                </td>
                {columns.map(col => {
                  const value = row[col];
                  const isPk = schemaMap[col]?.primary_key;
                  const isEditing = editingCell?.row === rowIdx && editingCell?.col === col;
                  const isSaving = savingCell?.row === rowIdx && savingCell?.col === col;

                  if (isSaving) {
                    return (
                      <td key={col} className="px-3 py-1.5 border-b border-gray-700/50">
                        <Loader2 className="w-3 h-3 animate-spin text-blue-400" />
                      </td>
                    );
                  }

                  if (isEditing) {
                    return (
                      <td key={col} className="px-1 py-0.5 border-b border-gray-700/50">
                        <input
                          type="text"
                          value={editValue}
                          onChange={e => onEditValueChange(e.target.value)}
                          onKeyDown={e => {
                            if (e.key === 'Enter') onCommitEdit();
                            if (e.key === 'Escape') onCancelEdit();
                          }}
                          onBlur={onCommitEdit}
                          className="w-full bg-gray-900 text-white text-xs rounded px-2 py-1 border border-blue-500 outline-none"
                          autoFocus
                        />
                      </td>
                    );
                  }

                  return (
                    <td
                      key={col}
                      className={`px-3 py-1.5 border-b border-gray-700/50 text-xs font-mono max-w-[300px] truncate ${
                        isPk ? 'text-gray-500 cursor-default' : 'text-gray-300 cursor-pointer hover:bg-gray-700/50'
                      }`}
                      title={value == null ? 'null' : String(value)}
                      onDoubleClick={() => !isPk && onStartEdit(rowIdx, col, value)}
                    >
                      {value == null ? <span className="italic text-gray-600">null</span> : String(value)}
                    </td>
                  );
                })}
              </tr>
            );
          })}
          {rows.length === 0 && (
            <tr>
              <td colSpan={columns.length + 1} className="text-center py-8 text-gray-500 text-xs">
                Aucune ligne
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
