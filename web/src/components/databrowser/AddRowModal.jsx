import { useState } from 'react';
import { X, Loader2 } from 'lucide-react';
import { getFieldComponent } from './fields';

const SYSTEM_COLUMNS = ['id', 'created_at', 'updated_at'];

export default function AddRowModal({ columns, lookupResolver, tableName, onClose, onAdd }) {
  const editableColumns = columns.filter(c => !SYSTEM_COLUMNS.includes(c.name) && c.field_type !== 'auto_increment');
  const [values, setValues] = useState(() => {
    const init = {};
    editableColumns.forEach(c => {
      init[c.name] = c.field_type === 'boolean' ? false : '';
    });
    return init;
  });
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState(null);

  const handleSubmit = async (e) => {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      // Clean up empty strings to null
      const cleaned = {};
      for (const [k, v] of Object.entries(values)) {
        cleaned[k] = v === '' ? null : v;
      }
      await onAdd(cleaned);
      onClose();
    } catch (err) {
      setError(err.response?.data?.error || 'Erreur lors de l\'insertion.');
      console.error('Insert failed:', err);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50" onClick={onClose}>
      <div className="bg-gray-800 border border-gray-700 rounded-xl shadow-xl w-full max-w-lg max-h-[85vh] flex flex-col" onClick={e => e.stopPropagation()}>
        <div className="flex items-center justify-between px-5 py-3.5 border-b border-gray-700">
          <h2 className="text-lg font-semibold text-gray-200">Nouvel enregistrement</h2>
          <button onClick={onClose} className="text-gray-400 hover:text-gray-200 transition-colors">
            <X className="w-5 h-5" />
          </button>
        </div>

        {error && (
          <div className="mx-5 mt-3 px-3 py-2 bg-red-900/30 border border-red-800 rounded-lg text-sm text-red-300">
            {error}
          </div>
        )}

        <form onSubmit={handleSubmit} className="flex-1 overflow-y-auto px-5 py-4 space-y-4">
          {editableColumns.map((col, i) => {
            const FieldComponent = getFieldComponent(col.field_type);
            const relation = lookupResolver?.getRelation(tableName, col.name);

            const extraProps = {};
            if (col.field_type === 'choice' || col.field_type === 'multi_choice') {
              extraProps.choices = col.choices || [];
            }
            if (col.field_type === 'lookup' && relation) {
              extraProps.lookupInfo = { targetTable: relation.to_table, targetColumn: relation.to_column };
            }
            if (col.field_type === 'date' || col.field_type === 'time' || col.field_type === 'date_time') {
              extraProps.fieldType = col.field_type;
            }
            if (col.field_type === 'number' || col.field_type === 'decimal') {
              extraProps.fieldType = col.field_type;
            }

            return (
              <div key={col.name}>
                <label className="block mb-1.5">
                  <span className="text-sm font-medium text-gray-300">{col.name}</span>
                  {col.required && <span className="text-yellow-400 ml-1">*</span>}
                  <span className="text-xs text-gray-600 ml-2">{col.field_type}</span>
                </label>
                <FieldComponent
                  value={values[col.name]}
                  onChange={v => setValues(prev => ({ ...prev, [col.name]: v }))}
                  autoFocus={i === 0}
                  label={col.name}
                  required={col.required}
                  {...extraProps}
                />
              </div>
            );
          })}
        </form>

        <div className="flex justify-end gap-2 px-5 py-3.5 border-t border-gray-700">
          <button type="button" onClick={onClose} className="px-4 py-2 text-sm text-gray-400 hover:text-gray-200 bg-gray-700 hover:bg-gray-600 rounded-lg transition-colors">
            Annuler
          </button>
          <button
            onClick={handleSubmit}
            disabled={submitting}
            className="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-500 text-white rounded-lg disabled:opacity-50 flex items-center gap-2 transition-colors"
          >
            {submitting && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
            Ajouter
          </button>
        </div>
      </div>
    </div>
  );
}
