interface PaginationProps {
  totalCount: number
  pageSize: number
  currentPage: number
  onPageChange: (page: number) => void
  onPageSizeChange: (size: number) => void
}

const PAGE_SIZES = [25, 50, 100]

export function Pagination({ totalCount, pageSize, currentPage, onPageChange, onPageSizeChange }: PaginationProps) {
  const totalPages = Math.max(1, Math.ceil(totalCount / pageSize))
  const start = Math.min(currentPage * pageSize + 1, totalCount)
  const end = Math.min((currentPage + 1) * pageSize, totalCount)

  // Generate page numbers to show
  function getPageNumbers(): (number | 'ellipsis')[] {
    if (totalPages <= 7) {
      return Array.from({ length: totalPages }, (_, i) => i)
    }
    const pages: (number | 'ellipsis')[] = [0]
    if (currentPage > 2) pages.push('ellipsis')
    for (let i = Math.max(1, currentPage - 1); i <= Math.min(totalPages - 2, currentPage + 1); i++) {
      pages.push(i)
    }
    if (currentPage < totalPages - 3) pages.push('ellipsis')
    pages.push(totalPages - 1)
    return pages
  }

  return (
    <div className="flex items-center justify-between px-4 py-2.5 border-t border-white/5" style={{ background: '#1a1a35' }}>
      {/* Left: count info */}
      <div className="text-xs text-white/30">
        {totalCount === 0 ? (
          'No rows'
        ) : (
          <>
            <span className="text-white/50">{start}</span>-<span className="text-white/50">{end}</span> of{' '}
            <span className="text-white/50">{totalCount.toLocaleString()}</span> rows
          </>
        )}
      </div>

      {/* Center: page numbers */}
      <div className="flex items-center gap-1">
        <button
          onClick={() => onPageChange(currentPage - 1)}
          disabled={currentPage === 0}
          className="p-1 rounded text-white/30 hover:text-white/60 disabled:opacity-20 disabled:cursor-not-allowed transition-colors"
        >
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 19.5L8.25 12l7.5-7.5" />
          </svg>
        </button>

        {getPageNumbers().map((p, i) =>
          p === 'ellipsis' ? (
            <span key={`e${i}`} className="px-1 text-white/15 text-xs">
              ...
            </span>
          ) : (
            <button
              key={p}
              onClick={() => onPageChange(p)}
              className={`min-w-[28px] h-7 rounded text-xs font-medium transition-colors ${
                p === currentPage
                  ? 'bg-[#7c3aed]/20 text-[#a78bfa] border border-[#7c3aed]/30'
                  : 'text-white/30 hover:text-white/60 hover:bg-white/5 border border-transparent'
              }`}
            >
              {p + 1}
            </button>
          ),
        )}

        <button
          onClick={() => onPageChange(currentPage + 1)}
          disabled={currentPage >= totalPages - 1}
          className="p-1 rounded text-white/30 hover:text-white/60 disabled:opacity-20 disabled:cursor-not-allowed transition-colors"
        >
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M8.25 4.5l7.5 7.5-7.5 7.5" />
          </svg>
        </button>
      </div>

      {/* Right: page size selector */}
      <div className="flex items-center gap-2">
        <span className="text-xs text-white/20">Rows:</span>
        <select
          value={pageSize}
          onChange={(e) => onPageSizeChange(Number(e.target.value))}
          className="text-xs rounded border border-white/10 bg-white/5 text-white/60 px-2 py-1 focus:outline-none focus:border-[#7c3aed]/50"
        >
          {PAGE_SIZES.map((s) => (
            <option key={s} value={s} className="bg-[#1e1e3a] text-white">
              {s}
            </option>
          ))}
        </select>
      </div>
    </div>
  )
}
