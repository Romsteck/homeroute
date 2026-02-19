import React, { useRef, useEffect, useCallback } from 'react';

const BASE_CLASS = 'w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30 transition-colors';
const READONLY_CLASS = 'bg-gray-800/50 border-gray-700 text-gray-400 cursor-not-allowed';

export default function TextField({ value, onChange, readOnly, autoFocus, label, required, description }) {
  const textareaRef = useRef(null);
  const strValue = value != null ? String(value) : '';
  const useTextarea = strValue.length > 100 || strValue.includes('\n');

  const autoResize = useCallback(() => {
    const el = textareaRef.current;
    if (el) {
      el.style.height = 'auto';
      el.style.height = Math.min(el.scrollHeight, 400) + 'px';
    }
  }, []);

  useEffect(() => {
    if (useTextarea) {
      autoResize();
    }
  }, [strValue, useTextarea, autoResize]);

  const handleChange = (e) => {
    if (onChange) onChange(e.target.value);
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
      {useTextarea ? (
        <textarea
          ref={textareaRef}
          value={strValue}
          onChange={handleChange}
          onInput={autoResize}
          readOnly={readOnly}
          autoFocus={autoFocus}
          className={`${inputClass} resize-none`}
          rows={3}
        />
      ) : (
        <input
          type="text"
          value={strValue}
          onChange={handleChange}
          readOnly={readOnly}
          autoFocus={autoFocus}
          className={inputClass}
        />
      )}
      <div className="flex justify-between mt-1">
        {description ? (
          <p className="text-xs text-gray-500">{description}</p>
        ) : (
          <span />
        )}
        <span className="text-xs text-gray-500">{strValue.length}</span>
      </div>
    </div>
  );
}
