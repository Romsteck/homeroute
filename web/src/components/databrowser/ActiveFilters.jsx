import { X } from 'lucide-react';

export default function ActiveFilters({ filters, onRemove, onClearAll }) {
  if (!filters || filters.length === 0) return null;

  return (
    <div className="flex items-center gap-2 px-4 py-2 bg-gray-800/50 border-b border-gray-700 flex-wrap">
      {filters.map((f, i) => (
        <span
          key={i}
          className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs bg-blue-900/40 text-blue-300"
        >
          <span>{f.column} {f.displayOp} {f.displayValue}</span>
          <button
            onClick={() => onRemove(i)}
            className="hover:text-red-400 transition-colors"
          >
            <X className="w-3 h-3" />
          </button>
        </span>
      ))}
      <button
        onClick={onClearAll}
        className="text-xs text-gray-500 hover:text-gray-300 ml-auto transition-colors"
      >
        Effacer tout
      </button>
    </div>
  );
}
