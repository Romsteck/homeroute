import React from 'react';

export default function ReadOnlyField({ value, label, description }) {
  const displayValue = value != null && value !== '' ? String(value) : '\u2014';

  return (
    <div>
      {label && (
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {label}
        </label>
      )}
      <div className="w-full bg-gray-900 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-400 font-mono cursor-not-allowed select-all">
        {displayValue}
      </div>
      {description && (
        <p className="mt-1 text-xs text-gray-500">{description}</p>
      )}
    </div>
  );
}
