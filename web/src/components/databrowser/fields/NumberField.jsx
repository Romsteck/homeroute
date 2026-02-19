import React, { useState } from 'react';

const BASE_CLASS = 'w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30 transition-colors';
const READONLY_CLASS = 'bg-gray-800/50 border-gray-700 text-gray-400 cursor-not-allowed';

export default function NumberField({ value, onChange, readOnly, autoFocus, label, required, description, fieldType }) {
  const [focused, setFocused] = useState(false);
  const isDecimal = fieldType === 'decimal';
  const step = isDecimal ? 0.01 : 1;

  const rawValue = value != null ? value : '';

  const formatDisplay = (val) => {
    if (val === '' || val == null) return '';
    const num = Number(val);
    if (isNaN(num)) return String(val);
    try {
      return new Intl.NumberFormat('fr-FR', {
        minimumFractionDigits: isDecimal ? 2 : 0,
        maximumFractionDigits: isDecimal ? 6 : 0,
      }).format(num);
    } catch {
      return String(val);
    }
  };

  const displayValue = focused ? rawValue : formatDisplay(rawValue);

  const handleChange = (e) => {
    const v = e.target.value;
    if (onChange) onChange(v === '' ? null : v);
  };

  const handleFocus = () => setFocused(true);
  const handleBlur = () => setFocused(false);

  const inputClass = `${BASE_CLASS} ${readOnly ? READONLY_CLASS : ''}`;

  return (
    <div>
      {label && (
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {label}
          {required && <span className="text-red-400 ml-1">*</span>}
        </label>
      )}
      <input
        type={focused ? 'number' : 'text'}
        value={displayValue}
        onChange={handleChange}
        onFocus={handleFocus}
        onBlur={handleBlur}
        readOnly={readOnly}
        autoFocus={autoFocus}
        step={step}
        className={inputClass}
      />
      {description && (
        <p className="mt-1 text-xs text-gray-500">{description}</p>
      )}
    </div>
  );
}
