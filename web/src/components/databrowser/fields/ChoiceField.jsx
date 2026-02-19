import React from 'react';

const BASE_CLASS = 'w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30 transition-colors appearance-none cursor-pointer';
const READONLY_CLASS = 'bg-gray-800/50 border-gray-700 text-gray-400 cursor-not-allowed';

function choiceHue(str) {
  return str.split('').reduce((a, c) => a + c.charCodeAt(0), 0) % 360;
}

export default function ChoiceField({ value, onChange, readOnly, autoFocus, label, required, description, choices = [] }) {
  const strValue = value != null ? String(value) : '';

  const handleChange = (e) => {
    const v = e.target.value;
    if (onChange) onChange(v === '' ? null : v);
  };

  const selectClass = `${BASE_CLASS} ${readOnly ? READONLY_CLASS : ''}`;

  return (
    <div>
      {label && (
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {label}
          {required && <span className="text-red-400 ml-1">*</span>}
        </label>
      )}
      <div className="relative">
        {strValue && (
          <span
            className="absolute left-3 top-1/2 -translate-y-1/2 w-2 h-2 rounded-full pointer-events-none"
            style={{ backgroundColor: `hsl(${choiceHue(strValue)}, 60%, 50%)` }}
          />
        )}
        <select
          value={strValue}
          onChange={handleChange}
          disabled={readOnly}
          autoFocus={autoFocus}
          className={`${selectClass} ${strValue ? 'pl-7' : ''}`}
        >
          <option value="">{'\u2014'}</option>
          {choices.map((choice) => {
            const hue = choiceHue(choice);
            return (
              <option key={choice} value={choice}>
                {choice}
              </option>
            );
          })}
        </select>
        <div className="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none">
          <svg className="w-4 h-4 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
          </svg>
        </div>
      </div>
      {description && (
        <p className="mt-1 text-xs text-gray-500">{description}</p>
      )}
    </div>
  );
}
