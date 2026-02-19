import React, { useState, useRef, useEffect } from 'react';
import { Plus, X } from 'lucide-react';

export default function MultiChoiceField({ value, onChange, readOnly, label, required, description, choices = [] }) {
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const dropdownRef = useRef(null);

  // Parse value: can be array, JSON string of array, or comma-separated string
  const selectedValues = React.useMemo(() => {
    if (Array.isArray(value)) return value;
    if (value == null || value === '') return [];
    if (typeof value === 'string') {
      try {
        const parsed = JSON.parse(value);
        if (Array.isArray(parsed)) return parsed;
      } catch {
        // not JSON
      }
      return value.split(',').map((s) => s.trim()).filter(Boolean);
    }
    return [];
  }, [value]);

  const remainingChoices = choices.filter((c) => !selectedValues.includes(c));

  useEffect(() => {
    function handleClickOutside(e) {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target)) {
        setDropdownOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const emitChange = (newValues) => {
    if (onChange) onChange(newValues);
  };

  const handleRemove = (item) => {
    if (readOnly) return;
    emitChange(selectedValues.filter((v) => v !== item));
  };

  const handleAdd = (item) => {
    emitChange([...selectedValues, item]);
    setDropdownOpen(false);
  };

  return (
    <div>
      {label && (
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {label}
          {required && <span className="text-red-400 ml-1">*</span>}
        </label>
      )}
      <div className="flex flex-wrap items-center gap-1.5 min-h-[38px] w-full bg-gray-700 border border-gray-600 rounded-lg px-2 py-1.5">
        {selectedValues.map((item) => (
          <span
            key={item}
            className="inline-flex items-center gap-1 bg-blue-900/40 text-blue-300 rounded-full px-2.5 py-0.5 text-sm"
          >
            {item}
            {!readOnly && (
              <button
                type="button"
                onClick={() => handleRemove(item)}
                className="hover:text-blue-100 transition-colors"
              >
                <X className="w-3 h-3" />
              </button>
            )}
          </span>
        ))}
        {!readOnly && remainingChoices.length > 0 && (
          <div className="relative" ref={dropdownRef}>
            <button
              type="button"
              onClick={() => setDropdownOpen(!dropdownOpen)}
              className="inline-flex items-center justify-center w-6 h-6 rounded-full bg-gray-600 hover:bg-gray-500 text-gray-300 transition-colors"
            >
              <Plus className="w-3.5 h-3.5" />
            </button>
            {dropdownOpen && (
              <div className="absolute z-50 top-full left-0 mt-1 w-48 max-h-48 overflow-y-auto bg-gray-700 border border-gray-600 rounded-lg shadow-lg py-1">
                {remainingChoices.map((choice) => (
                  <button
                    key={choice}
                    type="button"
                    onClick={() => handleAdd(choice)}
                    className="w-full text-left px-3 py-1.5 text-sm text-gray-200 hover:bg-gray-600 transition-colors"
                  >
                    {choice}
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
        {selectedValues.length === 0 && (
          <span className="text-sm text-gray-500 px-1">Aucune valeur</span>
        )}
      </div>
      {description && (
        <p className="mt-1 text-xs text-gray-500">{description}</p>
      )}
    </div>
  );
}
