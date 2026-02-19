import { useState, useEffect, useCallback, useImperativeHandle, forwardRef } from 'react';
import { Loader2, AlertCircle } from 'lucide-react';
import { getDataverseRows, updateDataverseRows, deleteDataverseRows } from '../../api/client';
import { getFieldComponent } from './fields';
import ReadOnlyField from './fields/ReadOnlyField';
import LookupLink from './LookupLink';

const SYSTEM_COLUMNS = ['id', 'created_at', 'updated_at'];

const RecordForm = forwardRef(function RecordForm({
  appId,
  tableName,
  recordId,
  columns,
  lookupResolver,
  onBack,
  onLookupNavigate,
  onDeleted,
}, ref) {
  const [originalRow, setOriginalRow] = useState(null);
  const [editValues, setEditValues] = useState({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState(null);
  const [notFound, setNotFound] = useState(false);

  const editableColumns = columns.filter(c => !SYSTEM_COLUMNS.includes(c.name) && c.field_type !== 'auto_increment');

  // Check dirty state
  const isDirty = originalRow && Object.keys(editValues).some(k => {
    const orig = originalRow[k];
    const edit = editValues[k];
    if (orig === null && edit === '') return false;
    if (orig === null && edit === null) return false;
    return String(orig) !== String(edit);
  });

  // Fetch single row
  const fetchRow = useCallback(async () => {
    if (!appId || !tableName || !recordId) return;
    setLoading(true);
    setError(null);
    setNotFound(false);
    try {
      const res = await getDataverseRows(appId, tableName, {
        limit: 1,
        offset: 0,
        filters: JSON.stringify([{ column: 'id', op: 'eq', value: Number(recordId) }]),
      });
      const rows = res.data?.data?.rows || [];
      if (rows.length === 0) {
        setNotFound(true);
        setOriginalRow(null);
      } else {
        const row = rows[0];
        setOriginalRow(row);
        // Initialize edit values with current row values for editable columns
        const init = {};
        editableColumns.forEach(c => {
          init[c.name] = row[c.name] ?? (c.field_type === 'boolean' ? false : '');
        });
        setEditValues(init);
      }
    } catch (err) {
      setError('Erreur lors du chargement de l\'enregistrement.');
      console.error('Fetch row failed:', err);
    } finally {
      setLoading(false);
    }
  }, [appId, tableName, recordId]);

  useEffect(() => { fetchRow(); }, [fetchRow]);

  async function handleSave() {
    if (!isDirty || saving) return;
    setSaving(true);
    setError(null);
    try {
      // Compute diff: only changed fields
      const updates = {};
      for (const key of Object.keys(editValues)) {
        const orig = originalRow[key];
        const edit = editValues[key];
        if (orig === null && edit === '') continue;
        if (String(orig) !== String(edit)) {
          updates[key] = edit === '' ? null : edit;
        }
      }

      if (Object.keys(updates).length === 0) {
        setSaving(false);
        return;
      }

      await updateDataverseRows(appId, tableName, {
        updates,
        filters: [{ column: 'id', op: 'eq', value: Number(recordId) }],
      });

      // Re-fetch to get updated timestamps
      await fetchRow();
    } catch (err) {
      setError('Erreur lors de la sauvegarde.');
      console.error('Save failed:', err);
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete() {
    if (!window.confirm('Supprimer cet enregistrement ? Cette action est irreversible.')) return;
    try {
      await deleteDataverseRows(appId, tableName, [{ column: 'id', op: 'eq', value: Number(recordId) }]);
      onDeleted();
    } catch (err) {
      setError('Erreur lors de la suppression.');
      console.error('Delete failed:', err);
    }
  }

  // Expose methods + state to parent via ref
  useImperativeHandle(ref, () => ({
    handleSave,
    handleDelete,
    isDirty: !!isDirty,
    saving,
  }), [isDirty, saving, editValues, originalRow]);

  // Ctrl+S to save
  useEffect(() => {
    const handler = (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        if (isDirty && !saving) handleSave();
      }
      if (e.key === 'Escape') {
        onBack();
      }
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [isDirty, saving, editValues, originalRow]);

  function handleFieldChange(columnName, value) {
    setEditValues(prev => ({ ...prev, [columnName]: value }));
  }

  if (loading) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <Loader2 className="w-8 h-8 text-blue-400 animate-spin" />
      </div>
    );
  }

  if (notFound) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center">
          <AlertCircle className="w-12 h-12 text-gray-600 mx-auto mb-3" />
          <p className="text-gray-400 text-lg">Enregistrement introuvable</p>
          <p className="text-gray-500 text-sm mt-1">L'enregistrement #{recordId} n'existe pas dans {tableName}.</p>
          <button onClick={onBack} className="mt-4 px-4 py-2 text-sm bg-gray-700 hover:bg-gray-600 text-gray-200 rounded-lg transition-colors">
            Retour a la liste
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto">
      {/* Form header */}
      <div className="bg-gray-800/50 border-b border-gray-700 px-6 py-4">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-2xl font-bold text-gray-100">
              <span className="text-blue-400">#{originalRow?.id}</span>
            </h2>
            <p className="text-sm text-gray-500 mt-0.5">{tableName}</p>
          </div>
          <div className="flex items-center gap-6 text-xs text-gray-500">
            <div>
              <span className="text-gray-600">Cree le</span>
              <p className="text-gray-400">{formatTimestamp(originalRow?.created_at)}</p>
            </div>
            <div>
              <span className="text-gray-600">Modifie le</span>
              <p className="text-gray-400">{formatTimestamp(originalRow?.updated_at)}</p>
            </div>
          </div>
        </div>
      </div>

      {/* Error banner */}
      {error && (
        <div className="mx-6 mt-4 px-4 py-2 bg-red-900/30 border border-red-800 rounded-lg text-sm text-red-300 flex items-center gap-2">
          <AlertCircle className="w-4 h-4 flex-shrink-0" />
          {error}
        </div>
      )}

      {/* Form fields */}
      <div className="px-6 py-6">
        {/* System fields (read-only) */}
        <div className="grid grid-cols-3 gap-4 mb-6 p-4 bg-gray-800/30 rounded-lg border border-gray-700/50">
          <ReadOnlyField label="ID" value={originalRow?.id} />
          <ReadOnlyField label="Cree le" value={formatTimestamp(originalRow?.created_at)} />
          <ReadOnlyField label="Modifie le" value={formatTimestamp(originalRow?.updated_at)} />
        </div>

        {/* Editable fields */}
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-x-6 gap-y-4">
          {editableColumns.map(col => {
            const FieldComponent = getFieldComponent(col.field_type);
            const relation = lookupResolver?.getRelation(tableName, col.name);

            // Build extra props for special field types
            const extraProps = {};
            if (col.field_type === 'choice' || col.field_type === 'multi_choice') {
              extraProps.choices = col.choices || [];
            }
            if (col.field_type === 'lookup' && relation) {
              extraProps.lookupInfo = { targetTable: relation.to_table, targetColumn: relation.to_column };
              extraProps.onLookupNavigate = (table, id) => onLookupNavigate(table, id);
            }
            if (col.field_type === 'date' || col.field_type === 'time' || col.field_type === 'date_time') {
              extraProps.fieldType = col.field_type;
            }
            if (col.field_type === 'number' || col.field_type === 'decimal') {
              extraProps.fieldType = col.field_type;
            }

            // JSON fields and textarea-like fields take full width
            const fullWidth = col.field_type === 'json' || col.field_type === 'text';

            return (
              <div key={col.name} className={fullWidth ? 'lg:col-span-2' : ''}>
                <label className="block mb-1.5">
                  <span className="text-sm font-medium text-gray-300">{col.name}</span>
                  {col.required && <span className="text-yellow-400 ml-1">*</span>}
                  <span className="text-xs text-gray-600 ml-2">{col.field_type}</span>
                </label>
                {col.description && (
                  <p className="text-xs text-gray-500 mb-1.5">{col.description}</p>
                )}
                <FieldComponent
                  value={editValues[col.name]}
                  onChange={(v) => handleFieldChange(col.name, v)}
                  readOnly={false}
                  label={col.name}
                  required={col.required}
                  {...extraProps}
                />
                {/* Lookup preview link */}
                {col.field_type === 'lookup' && relation && editValues[col.name] && (
                  <div className="mt-1">
                    <LookupLink
                      targetTable={relation.to_table}
                      value={editValues[col.name]}
                      onClick={(table, id) => onLookupNavigate(table, id)}
                    />
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
});

export default RecordForm;

function formatTimestamp(v) {
  if (!v) return '—';
  try {
    const d = new Date(v);
    return d.toLocaleDateString('fr-FR') + ' ' + d.toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
  } catch {
    return String(v);
  }
}
