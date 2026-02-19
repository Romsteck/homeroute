import { useState, useEffect, useRef } from 'react';

const OPERATORS_BY_TYPE = {
  text:           [{ label: 'Contient', op: 'like', wild: 'contains' }, { label: 'Egal à', op: 'eq' }, { label: 'Commence par', op: 'like', wild: 'starts' }, { label: 'Est vide', op: 'is_null' }],
  email:          [{ label: 'Contient', op: 'like', wild: 'contains' }, { label: 'Egal à', op: 'eq' }, { label: 'Commence par', op: 'like', wild: 'starts' }, { label: 'Est vide', op: 'is_null' }],
  url:            [{ label: 'Contient', op: 'like', wild: 'contains' }, { label: 'Egal à', op: 'eq' }, { label: 'Commence par', op: 'like', wild: 'starts' }, { label: 'Est vide', op: 'is_null' }],
  phone:          [{ label: 'Contient', op: 'like', wild: 'contains' }, { label: 'Egal à', op: 'eq' }, { label: 'Commence par', op: 'like', wild: 'starts' }, { label: 'Est vide', op: 'is_null' }],
  number:         [{ label: '=', op: 'eq' }, { label: '≠', op: 'ne' }, { label: '>', op: 'gt' }, { label: '<', op: 'lt' }, { label: '≥', op: 'gte' }, { label: '≤', op: 'lte' }],
  decimal:        [{ label: '=', op: 'eq' }, { label: '≠', op: 'ne' }, { label: '>', op: 'gt' }, { label: '<', op: 'lt' }, { label: '≥', op: 'gte' }, { label: '≤', op: 'lte' }],
  currency:       [{ label: '=', op: 'eq' }, { label: '≠', op: 'ne' }, { label: '>', op: 'gt' }, { label: '<', op: 'lt' }, { label: '≥', op: 'gte' }, { label: '≤', op: 'lte' }],
  percent:        [{ label: '=', op: 'eq' }, { label: '≠', op: 'ne' }, { label: '>', op: 'gt' }, { label: '<', op: 'lt' }, { label: '≥', op: 'gte' }, { label: '≤', op: 'lte' }],
  auto_increment: [{ label: '=', op: 'eq' }, { label: '≠', op: 'ne' }, { label: '>', op: 'gt' }, { label: '<', op: 'lt' }, { label: '≥', op: 'gte' }, { label: '≤', op: 'lte' }],
  boolean:        [{ label: 'Est vrai', op: 'eq', value: true }, { label: 'Est faux', op: 'eq', value: false }],
  date:           [{ label: 'Egal', op: 'eq' }, { label: 'Avant', op: 'lt' }, { label: 'Après', op: 'gt' }],
  time:           [{ label: 'Egal', op: 'eq' }, { label: 'Avant', op: 'lt' }, { label: 'Après', op: 'gt' }],
  date_time:      [{ label: 'Egal', op: 'eq' }, { label: 'Avant', op: 'lt' }, { label: 'Après', op: 'gt' }],
  choice:         [{ label: 'Est', op: 'eq' }],
  lookup:         [{ label: '=', op: 'eq' }],
};

function getOperators(fieldType) {
  return OPERATORS_BY_TYPE[fieldType] || OPERATORS_BY_TYPE.text;
}

function isNumericType(type) {
  return ['number', 'decimal', 'currency', 'percent', 'auto_increment'].includes(type);
}

function isDateType(type) {
  return ['date', 'time', 'date_time'].includes(type);
}

export default function ColumnFilter({ column, fieldType, choices, currentFilter, onApply, onClear, onClose }) {
  const operators = getOperators(fieldType);
  const ref = useRef(null);

  // Derive initial selected operator index from currentFilter
  const initOpIdx = () => {
    if (!currentFilter) return 0;
    const idx = operators.findIndex(o => o.op === currentFilter.op);
    return idx >= 0 ? idx : 0;
  };

  const [selectedOpIdx, setSelectedOpIdx] = useState(initOpIdx);
  const [value, setValue] = useState(currentFilter?.value ?? '');

  const selectedOp = operators[selectedOpIdx];
  const isBoolean = fieldType === 'boolean';
  const isChoice = fieldType === 'choice';
  const isLookup = fieldType === 'lookup';
  const needsNoInput = isBoolean || selectedOp?.op === 'is_null';

  // Close on click outside
  useEffect(() => {
    function handleClick(e) {
      if (ref.current && !ref.current.contains(e.target)) {
        onClose();
      }
    }
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [onClose]);

  function handleApply() {
    const op = selectedOp;
    let finalValue = value;

    if (isBoolean) {
      finalValue = op.value;
    } else if (op.op === 'is_null') {
      finalValue = null;
    } else if (op.wild === 'contains') {
      finalValue = `%${value}%`;
    } else if (op.wild === 'starts') {
      finalValue = `${value}%`;
    } else if (isNumericType(fieldType) || isLookup) {
      finalValue = Number(value);
    }

    onApply({ column, op: op.op, value: finalValue });
  }

  function renderInput() {
    if (needsNoInput) return null;

    if (isChoice && choices?.length > 0) {
      return (
        <select
          className="w-full bg-gray-900 border border-gray-600 rounded px-2 py-1.5 text-sm text-gray-200 focus:outline-none focus:border-blue-500"
          value={value}
          onChange={e => setValue(e.target.value)}
        >
          <option value="">-- Choisir --</option>
          {choices.map(c => (
            <option key={c} value={c}>{c}</option>
          ))}
        </select>
      );
    }

    if (isNumericType(fieldType) || isLookup) {
      return (
        <input
          type="number"
          className="w-full bg-gray-900 border border-gray-600 rounded px-2 py-1.5 text-sm text-gray-200 focus:outline-none focus:border-blue-500"
          value={value}
          onChange={e => setValue(e.target.value)}
          placeholder="Valeur..."
        />
      );
    }

    if (isDateType(fieldType)) {
      const inputType = fieldType === 'time' ? 'time' : fieldType === 'date_time' ? 'datetime-local' : 'date';
      return (
        <input
          type={inputType}
          className="w-full bg-gray-900 border border-gray-600 rounded px-2 py-1.5 text-sm text-gray-200 focus:outline-none focus:border-blue-500"
          value={value}
          onChange={e => setValue(e.target.value)}
        />
      );
    }

    // Default: text input
    return (
      <input
        type="text"
        className="w-full bg-gray-900 border border-gray-600 rounded px-2 py-1.5 text-sm text-gray-200 focus:outline-none focus:border-blue-500"
        value={value}
        onChange={e => setValue(e.target.value)}
        placeholder="Valeur..."
      />
    );
  }

  return (
    <div
      ref={ref}
      className="absolute top-full left-0 mt-1 bg-gray-800 border border-gray-700 rounded-lg shadow-xl p-3 min-w-[220px] z-30"
    >
      {/* Column name */}
      <div className="font-medium text-gray-300 text-sm mb-2">{column}</div>

      {/* Operator select */}
      <select
        className="w-full bg-gray-900 border border-gray-600 rounded px-2 py-1.5 text-sm text-gray-200 mb-2 focus:outline-none focus:border-blue-500"
        value={selectedOpIdx}
        onChange={e => setSelectedOpIdx(Number(e.target.value))}
      >
        {operators.map((op, i) => (
          <option key={i} value={i}>{op.label}</option>
        ))}
      </select>

      {/* Value input */}
      {renderInput() && <div className="mb-3">{renderInput()}</div>}

      {/* Buttons */}
      <div className="flex items-center gap-2">
        <button
          onClick={handleApply}
          className="flex-1 px-3 py-1.5 text-xs font-medium bg-blue-600 hover:bg-blue-500 text-white rounded transition-colors"
        >
          Appliquer
        </button>
        {currentFilter && (
          <button
            onClick={onClear}
            className="flex-1 px-3 py-1.5 text-xs font-medium bg-gray-700 hover:bg-gray-600 text-gray-300 rounded transition-colors"
          >
            Effacer
          </button>
        )}
      </div>
    </div>
  );
}
