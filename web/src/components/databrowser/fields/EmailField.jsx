import React from 'react';
import { Mail } from 'lucide-react';

const BASE_CLASS = 'w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30 transition-colors';
const READONLY_CLASS = 'bg-gray-800/50 border-gray-700 text-gray-400 cursor-not-allowed';

const EMAIL_REGEX = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export default function EmailField({ value, onChange, readOnly, autoFocus, label, required, description }) {
  const strValue = value != null ? String(value) : '';
  const isValid = strValue === '' || EMAIL_REGEX.test(strValue);
  const showMailButton = strValue !== '' && isValid;
  const showError = strValue !== '' && !isValid;

  const handleChange = (e) => {
    if (onChange) onChange(e.target.value);
  };

  const inputClass = [
    BASE_CLASS,
    readOnly ? READONLY_CLASS : '',
    showMailButton ? 'pr-10' : '',
    showError ? 'border-red-500 focus:border-red-500 focus:ring-red-500/30' : '',
  ].filter(Boolean).join(' ');

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
          type="email"
          value={strValue}
          onChange={handleChange}
          readOnly={readOnly}
          autoFocus={autoFocus}
          className={inputClass}
        />
        {showMailButton && (
          <a
            href={`mailto:${strValue}`}
            className="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-gray-400 hover:text-blue-400 transition-colors rounded"
            title="Envoyer un email"
          >
            <Mail className="w-4 h-4" />
          </a>
        )}
      </div>
      {showError && (
        <p className="mt-1 text-xs text-red-400">Email invalide</p>
      )}
      {description && !showError && (
        <p className="mt-1 text-xs text-gray-500">{description}</p>
      )}
    </div>
  );
}
