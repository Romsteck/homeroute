import React from 'react';

export default function BooleanField({ value, onChange, readOnly, label, required, description }) {
  const isOn = value === true || value === 1 || value === '1' || value === 'true';

  const handleToggle = () => {
    if (readOnly) return;
    if (onChange) onChange(!isOn);
  };

  const handleKeyDown = (e) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleToggle();
    }
  };

  return (
    <div>
      {label && (
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {label}
          {required && <span className="text-red-400 ml-1">*</span>}
        </label>
      )}
      <div className="flex items-center gap-3">
        <button
          type="button"
          role="switch"
          aria-checked={isOn}
          onClick={handleToggle}
          onKeyDown={handleKeyDown}
          disabled={readOnly}
          className={`
            relative inline-flex h-6 w-11 flex-shrink-0 rounded-full border-2 border-transparent
            transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-blue-500/30
            ${isOn ? 'bg-blue-500' : 'bg-gray-600'}
            ${readOnly ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}
          `}
        >
          <span
            className={`
              pointer-events-none inline-block h-5 w-5 rounded-full bg-white shadow-sm
              transform transition-transform duration-200 ease-in-out
              ${isOn ? 'translate-x-5' : 'translate-x-0'}
            `}
          />
        </button>
        <span className="text-sm text-gray-300">
          {isOn ? 'Oui' : 'Non'}
        </span>
      </div>
      {description && (
        <p className="mt-1 text-xs text-gray-500">{description}</p>
      )}
    </div>
  );
}
