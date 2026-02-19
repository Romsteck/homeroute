import React, { useState, useRef, useEffect, useCallback } from 'react';

const BASE_CLASS = 'w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-sm text-gray-200 font-mono focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30 transition-colors resize-none';
const READONLY_CLASS = 'bg-gray-800/50 border-gray-700 text-gray-400 cursor-not-allowed';

function tryPrettyPrint(val) {
  if (val == null || val === '') return '';
  const str = typeof val === 'string' ? val : JSON.stringify(val);
  try {
    return JSON.stringify(JSON.parse(str), null, 2);
  } catch {
    return str;
  }
}

function countLines(str) {
  if (!str) return 1;
  return str.split('\n').length;
}

function clampRows(lineCount) {
  return Math.max(3, Math.min(20, lineCount));
}

export default function JsonField({ value, onChange, readOnly, autoFocus, label, required, description }) {
  const [text, setText] = useState(() => tryPrettyPrint(value));
  const [jsonError, setJsonError] = useState(null);
  const textareaRef = useRef(null);

  // Sync external value changes
  useEffect(() => {
    setText(tryPrettyPrint(value));
  }, [value]);

  const autoResize = useCallback(() => {
    const el = textareaRef.current;
    if (el) {
      el.style.height = 'auto';
      el.style.height = Math.min(el.scrollHeight, 500) + 'px';
    }
  }, []);

  useEffect(() => {
    autoResize();
  }, [text, autoResize]);

  const handleChange = (e) => {
    const v = e.target.value;
    setText(v);
    setJsonError(null);
    if (onChange) onChange(v);
  };

  const handleBlur = () => {
    if (text.trim() === '') {
      setJsonError(null);
      return;
    }
    try {
      JSON.parse(text);
      setJsonError(null);
    } catch (e) {
      setJsonError('JSON invalide');
    }
  };

  const lineCount = countLines(text);
  const rows = clampRows(lineCount);

  const inputClass = [
    BASE_CLASS,
    readOnly ? READONLY_CLASS : '',
    jsonError ? 'border-red-500 focus:border-red-500 focus:ring-red-500/30' : '',
  ].filter(Boolean).join(' ');

  return (
    <div>
      {label && (
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {label}
          {required && <span className="text-red-400 ml-1">*</span>}
        </label>
      )}
      <textarea
        ref={textareaRef}
        value={text}
        onChange={handleChange}
        onBlur={handleBlur}
        onInput={autoResize}
        readOnly={readOnly}
        autoFocus={autoFocus}
        rows={rows}
        className={inputClass}
        spellCheck={false}
      />
      <div className="flex justify-between mt-1">
        <span className="text-xs text-gray-600">{lineCount} ligne{lineCount > 1 ? 's' : ''}</span>
        {jsonError && (
          <span className="text-xs text-red-400">{jsonError}</span>
        )}
      </div>
      {description && (
        <p className="mt-1 text-xs text-gray-500">{description}</p>
      )}
    </div>
  );
}
