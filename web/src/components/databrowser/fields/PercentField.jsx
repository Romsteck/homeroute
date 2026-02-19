import React from 'react';

const BASE_CLASS = 'w-full bg-gray-700 border border-gray-600 rounded-lg pl-3 pr-8 py-2 text-sm text-gray-200 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30 transition-colors';
const READONLY_CLASS = 'bg-gray-800/50 border-gray-700 text-gray-400 cursor-not-allowed';

function getBarColor(val) {
  if (val < 30) return 'bg-red-500';
  if (val < 70) return 'bg-yellow-500';
  return 'bg-green-500';
}

export default function PercentField({ value, onChange, readOnly, autoFocus, label, required, description }) {
  const rawValue = value != null ? value : '';
  const numValue = Number(rawValue);
  const hasValue = rawValue !== '' && !isNaN(numValue);
  const clampedValue = hasValue ? Math.max(0, Math.min(100, numValue)) : 0;

  const handleChange = (e) => {
    const v = e.target.value;
    if (onChange) onChange(v === '' ? null : v);
  };

  const inputClass = `${BASE_CLASS} ${readOnly ? READONLY_CLASS : ''}`;

  return (
    <div>
      {label && (
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {label}
          {required && <span className="text-red-400 ml-1">*</span>}
        </label>
      )}
      <div className="relative">
        <input
          type="number"
          value={rawValue}
          onChange={handleChange}
          readOnly={readOnly}
          autoFocus={autoFocus}
          step={1}
          className={inputClass}
        />
        <span className="absolute right-3 top-1/2 -translate-y-1/2 text-sm text-gray-400 pointer-events-none select-none">
          %
        </span>
      </div>
      {hasValue && (
        <div className="mt-1.5 h-1 bg-gray-600 rounded-full overflow-hidden">
          <div
            className={`h-full rounded-full transition-all duration-300 ${getBarColor(clampedValue)}`}
            style={{ width: `${clampedValue}%` }}
          />
        </div>
      )}
      {description && (
        <p className="mt-1 text-xs text-gray-500">{description}</p>
      )}
    </div>
  );
}
