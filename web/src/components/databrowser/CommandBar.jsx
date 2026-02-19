import { Plus, RefreshCw, Download, ArrowLeft, Trash2, Save, Loader2 } from 'lucide-react';
import Breadcrumb from './Breadcrumb';

function CmdButton({ icon: Icon, label, onClick, disabled, variant = 'default', spinning }) {
  const base = 'flex items-center gap-2 px-4 py-2 text-xs font-medium transition-colors border-r border-gray-700/50 last:border-r-0';
  const variants = {
    default: 'text-gray-300 hover:bg-gray-700/60 hover:text-white',
    primary: 'text-blue-400 hover:bg-blue-900/30 hover:text-blue-300',
    danger: 'text-red-400 hover:bg-red-900/30 hover:text-red-300',
    disabled: 'text-gray-600 cursor-not-allowed',
  };
  const v = disabled ? 'disabled' : variant;

  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`${base} ${variants[v]}`}
    >
      {spinning ? (
        <Loader2 className="w-4.5 h-4.5 animate-spin" />
      ) : (
        <Icon className="w-4.5 h-4.5" />
      )}
      <span>{label}</span>
    </button>
  );
}

function GridCommands({ onAddRow, onRefresh, onBackup, totalRows, rowsPerPage, onRowsPerPageChange }) {
  return (
    <>
      <CmdButton icon={Plus} label="Nouveau" onClick={onAddRow} variant="primary" />
      <CmdButton icon={RefreshCw} label="Actualiser" onClick={onRefresh} />
      <CmdButton icon={Download} label="Backup" onClick={onBackup} />

      <div className="flex items-center gap-3 ml-auto px-3">
        {totalRows != null && (
          <span className="text-xs text-gray-400">
            {totalRows} ligne{totalRows !== 1 ? 's' : ''}
          </span>
        )}
        {onRowsPerPageChange && (
          <select
            className="bg-gray-800 border border-gray-600 rounded px-2 py-1 text-xs text-gray-300 focus:outline-none focus:border-blue-500"
            value={rowsPerPage}
            onChange={e => onRowsPerPageChange(Number(e.target.value))}
          >
            {[25, 50, 100, 200].map(n => (
              <option key={n} value={n}>{n} / page</option>
            ))}
          </select>
        )}
      </div>
    </>
  );
}

function FormCommands({ onBack, onDelete, onSave, isDirty, isSaving }) {
  return (
    <>
      <CmdButton icon={ArrowLeft} label="Retour" onClick={onBack} />
      <CmdButton icon={Save} label="Sauvegarder" onClick={onSave} disabled={!isDirty || isSaving} variant="primary" spinning={isSaving} />
      <CmdButton icon={Trash2} label="Supprimer" onClick={onDelete} variant="danger" />

      {isDirty && (
        <div className="flex items-center ml-auto px-3">
          <span className="text-xs text-yellow-400 bg-yellow-900/30 px-2 py-0.5 rounded">Non sauvegardé</span>
        </div>
      )}
    </>
  );
}

export default function CommandBar({
  viewMode,
  appSlug,
  tableName,
  recordId,
  onNavigateHome,
  onNavigateApp,
  onNavigateTable,
  onAddRow,
  onRefresh,
  onBackup,
  totalRows,
  rowsPerPage,
  onRowsPerPageChange,
  onBack,
  onDelete,
  onSave,
  isDirty,
  isSaving,
}) {
  return (
    <div className="flex-shrink-0">
      {/* Row 1: Breadcrumb */}
      <div className="bg-gray-800/80 border-b border-gray-700/50 px-4 py-2">
        <Breadcrumb
          appSlug={appSlug}
          tableName={tableName}
          recordId={recordId}
          onNavigateHome={onNavigateHome}
          onNavigateApp={onNavigateApp}
          onNavigateTable={onNavigateTable}
        />
      </div>

      {/* Row 2: Command bar buttons */}
      {(viewMode === 'grid' || viewMode === 'form') && (
        <div className="bg-gray-800 border-b border-gray-700 px-1 flex items-stretch min-h-[48px]">
          {viewMode === 'grid' && (
            <GridCommands
              onAddRow={onAddRow}
              onRefresh={onRefresh}
              onBackup={onBackup}
              totalRows={totalRows}
              rowsPerPage={rowsPerPage}
              onRowsPerPageChange={onRowsPerPageChange}
            />
          )}
          {viewMode === 'form' && (
            <FormCommands
              onBack={onBack}
              onDelete={onDelete}
              onSave={onSave}
              isDirty={isDirty}
              isSaving={isSaving}
            />
          )}
        </div>
      )}
    </div>
  );
}
