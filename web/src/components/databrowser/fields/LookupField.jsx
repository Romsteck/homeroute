import React from 'react';
import { ExternalLink } from 'lucide-react';

const BASE_CLASS = 'w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30 transition-colors';
const READONLY_CLASS = 'bg-gray-800/50 border-gray-700 text-gray-400 cursor-not-allowed';

export default function LookupField({ value, onChange, readOnly, autoFocus, label, required, description, lookupInfo, onLookupNavigate }) {
  const rawValue = value != null ? value : '';
  const hasValue = rawValue !== '' && rawValue != null;
  const targetTable = lookupInfo?.targetTable;
  const targetColumn = lookupInfo?.targetColumn;

  const handleChange = (e) => {
    const v = e.target.value;
    if (onChange) onChange(v === '' ? null : Number(v));
  };

  const handleNavigate = (e) => {
    e.preventDefault();
    if (onLookupNavigate && targetTable && hasValue) {
      onLookupNavigate(targetTable, rawValue);
    }
  };

  const inputClass = [
    BASE_CLASS,
    readOnly ? READONLY_CLASS : '',
    hasValue ? 'pr-10' : '',
  ].filter(Boolean).join(' ');

  return (
    <div>
      {label && (
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {label}
          {required && <span className="text-red-400 ml-1">*</span>}
          {targetColumn && (
            <span className="ml-2 text-xs text-gray-500 font-normal">
              → {targetTable}.{targetColumn}
            </span>
          )}
        </label>
      )}
      <div className="flex items-center gap-2">
        <div className="relative flex-1">
          <input
            type="number"
            value={rawValue}
            onChange={handleChange}
            readOnly={readOnly}
            autoFocus={autoFocus}
            className={inputClass}
            step={1}
          />
        </div>
        {hasValue && targetTable && (
          <button
            type="button"
            onClick={handleNavigate}
            className="inline-flex items-center gap-1 text-sm text-blue-400 hover:text-blue-300 transition-colors whitespace-nowrap"
            title={`Voir ${targetTable} #${rawValue}`}
          >
            <span>{targetTable} #{rawValue}</span>
            <ExternalLink className="w-3.5 h-3.5" />
          </button>
        )}
      </div>
      {description && (
        <p className="mt-1 text-xs text-gray-500">{description}</p>
      )}
    </div>
  );
}
