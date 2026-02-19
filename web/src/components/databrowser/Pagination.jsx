import { ChevronsLeft, ChevronLeft, ChevronRight, ChevronsRight } from 'lucide-react';

export default function Pagination({ page, totalPages, onPageChange }) {
  const maxVisible = 7;
  let start = Math.max(1, page - Math.floor(maxVisible / 2));
  let end = Math.min(totalPages, start + maxVisible - 1);
  if (end - start + 1 < maxVisible) start = Math.max(1, end - maxVisible + 1);

  const pages = [];
  for (let i = start; i <= end; i++) pages.push(i);

  const btnClass = 'px-2 py-1 text-sm rounded transition-colors disabled:opacity-30';
  const activeClass = 'bg-blue-600 text-white';
  const inactiveClass = 'text-gray-400 hover:bg-gray-700';

  return (
    <div className="flex items-center justify-center gap-1">
      <button onClick={() => onPageChange(1)} disabled={page === 1} className={btnClass + ' ' + inactiveClass}>
        <ChevronsLeft className="w-4 h-4" />
      </button>
      <button onClick={() => onPageChange(page - 1)} disabled={page === 1} className={btnClass + ' ' + inactiveClass}>
        <ChevronLeft className="w-4 h-4" />
      </button>
      {start > 1 && <span className="text-gray-600 px-1">...</span>}
      {pages.map(p => (
        <button key={p} onClick={() => onPageChange(p)} className={`${btnClass} min-w-[28px] ${p === page ? activeClass : inactiveClass}`}>
          {p}
        </button>
      ))}
      {end < totalPages && <span className="text-gray-600 px-1">...</span>}
      <button onClick={() => onPageChange(page + 1)} disabled={page === totalPages} className={btnClass + ' ' + inactiveClass}>
        <ChevronRight className="w-4 h-4" />
      </button>
      <button onClick={() => onPageChange(totalPages)} disabled={page === totalPages} className={btnClass + ' ' + inactiveClass}>
        <ChevronsRight className="w-4 h-4" />
      </button>
    </div>
  );
}
