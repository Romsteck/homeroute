import { useState } from 'react';
import { X, Plus, Loader2 } from 'lucide-react';

export function AddRowModal({ columns, onInsert, onClose }) {
  const editableCols = (columns || []).filter(c => !c.primary_key);
  const [values, setValues] = useState(() => {
    const init = {};
    editableCols.forEach(c => { init[c.name] = ''; });
    return init;
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState(null);

  const handleSubmit = async (e) => {
    e.preventDefault();
    setSaving(true);
    setError(null);
    try {
      const row = {};
      editableCols.forEach(c => {
        const v = values[c.name];
        if (v === '' && !c.required) {
          row[c.name] = null;
        } else {
          row[c.name] = coerce(v, c.data_type || c.field_type || 'TEXT');
        }
      });
      await onInsert(row);
      onClose();
    } catch (err) {
      setError(err.message || 'Erreur');
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="bg-gray-800 rounded-lg border border-gray-700 shadow-xl w-full max-w-md max-h-[80vh] flex flex-col">
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-700">
          <h3 className="text-sm font-semibold text-white flex items-center gap-2">
            <Plus className="w-4 h-4 text-blue-400" /> Ajouter une ligne
          </h3>
          <button onClick={onClose} className="p-1 text-gray-400 hover:text-white rounded hover:bg-gray-700 border-none bg-transparent cursor-pointer">
            <X className="w-4 h-4" />
          </button>
        </div>
        <form onSubmit={handleSubmit} className="flex-1 overflow-y-auto p-4 space-y-3">
          {editableCols.map(col => (
            <div key={col.name}>
              <label className="block text-xs text-gray-400 mb-1">
                {col.name}
                {col.required && <span className="text-red-400 ml-1">*</span>}
                <span className="text-gray-600 ml-1">({col.data_type || col.field_type || '?'})</span>
              </label>
              <input
                type="text"
                value={values[col.name]}
                onChange={e => setValues(prev => ({ ...prev, [col.name]: e.target.value }))}
                required={col.required}
                className="w-full bg-gray-900 text-white text-sm rounded px-3 py-1.5 border border-gray-600 outline-none focus:border-blue-500"
                placeholder={col.required ? 'Requis' : 'Optionnel (null)'}
              />
            </div>
          ))}
          {error && <div className="text-xs text-red-400 bg-red-500/10 rounded px-3 py-2">{error}</div>}
        </form>
        <div className="flex justify-end gap-2 px-4 py-3 border-t border-gray-700">
          <button type="button" onClick={onClose} className="px-3 py-1.5 text-xs text-gray-400 rounded border-none bg-transparent cursor-pointer hover:text-white">
            Annuler
          </button>
          <button onClick={handleSubmit} disabled={saving} className="px-4 py-1.5 text-xs text-white bg-blue-500 rounded border-none cursor-pointer hover:bg-blue-600 disabled:opacity-50 flex items-center gap-1">
            {saving ? <><Loader2 className="w-3 h-3 animate-spin" /> Ajout...</> : <><Plus className="w-3 h-3" /> Ajouter</>}
          </button>
        </div>
      </div>
    </div>
  );
}

function coerce(value, type) {
  const t = (type || '').toLowerCase();
  if (['integer', 'int', 'bigint', 'smallint'].includes(t)) {
    const n = parseInt(value, 10);
    return isNaN(n) ? value : n;
  }
  if (['real', 'float', 'double', 'numeric', 'decimal'].includes(t)) {
    const n = parseFloat(value);
    return isNaN(n) ? value : n;
  }
  if (t === 'boolean' || t === 'bool') {
    return value === 'true' || value === '1';
  }
  return value;
}
