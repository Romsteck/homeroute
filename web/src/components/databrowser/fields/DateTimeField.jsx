import React from 'react';

const BASE_CLASS = 'w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30 transition-colors';
const READONLY_CLASS = 'bg-gray-800/50 border-gray-700 text-gray-400 cursor-not-allowed';

function formatReadOnly(value, fieldType) {
  if (value == null || value === '') return '\u2014';
  try {
    if (fieldType === 'date') {
      const d = new Date(value + 'T00:00:00');
      if (isNaN(d.getTime())) return String(value);
      return d.toLocaleDateString('fr-FR', { day: '2-digit', month: '2-digit', year: 'numeric' });
    }
    if (fieldType === 'time') {
      const parts = String(value).split(':');
      return `${(parts[0] || '').padStart(2, '0')}:${(parts[1] || '00').padStart(2, '0')}`;
    }
    // date_time
    const d = new Date(value);
    if (isNaN(d.getTime())) return String(value);
    const datePart = d.toLocaleDateString('fr-FR', { day: '2-digit', month: '2-digit', year: 'numeric' });
    const timePart = d.toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
    return `${datePart} ${timePart}`;
  } catch {
    return String(value);
  }
}

function getInputType(fieldType) {
  if (fieldType === 'date') return 'date';
  if (fieldType === 'time') return 'time';
  return 'datetime-local';
}

export default function DateTimeField({ value, onChange, readOnly, autoFocus, label, required, description, fieldType = 'date_time' }) {
  const strValue = value != null ? String(value) : '';

  const handleChange = (e) => {
    const v = e.target.value;
    if (onChange) onChange(v === '' ? null : v);
  };

  if (readOnly) {
    return (
      <div>
        {label && (
          <label className="block text-sm font-medium text-gray-300 mb-1">
            {label}
          </label>
        )}
        <div className="w-full bg-gray-800/50 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-400 cursor-not-allowed">
          {formatReadOnly(strValue, fieldType)}
        </div>
        {description && (
          <p className="mt-1 text-xs text-gray-500">{description}</p>
        )}
      </div>
    );
  }

  return (
    <div>
      {label && (
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {label}
          {required && <span className="text-red-400 ml-1">*</span>}
        </label>
      )}
      <input
        type={getInputType(fieldType)}
        value={strValue}
        onChange={handleChange}
        autoFocus={autoFocus}
        className={BASE_CLASS}
      />
      {description && (
        <p className="mt-1 text-xs text-gray-500">{description}</p>
      )}
    </div>
  );
}
