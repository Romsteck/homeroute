import { useState, useRef, useEffect, useCallback } from 'react';

function relativeTime(epochSecs) {
  if (!epochSecs) return '';
  const now = Math.floor(Date.now() / 1000);
  const diff = now - epochSecs;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 2592000) return `${Math.floor(diff / 86400)}d ago`;
  return new Date(epochSecs * 1000).toLocaleDateString();
}

function groupSessionsByDate(sessions) {
  const now = Math.floor(Date.now() / 1000);
  const today = now - (now % 86400);
  const yesterday = today - 86400;
  const weekAgo = today - 604800;

  const groups = { today: [], yesterday: [], week: [], older: [] };
  for (const s of sessions) {
    const t = s.last_modified || 0;
    if (t >= today) groups.today.push(s);
    else if (t >= yesterday) groups.yesterday.push(s);
    else if (t >= weekAgo) groups.week.push(s);
    else groups.older.push(s);
  }
  return groups;
}

export default function SessionPicker({ sessions, currentSessionId, onSelect, onNew, onDelete }) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState('');
  const [hoveredId, setHoveredId] = useState(null);
  const dropdownRef = useRef(null);
  const searchRef = useRef(null);

  // Close on click outside
  useEffect(() => {
    if (!open) return;
    const handler = (e) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target)) {
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  // Focus search when opened
  useEffect(() => {
    if (open && searchRef.current) {
      searchRef.current.focus();
    }
  }, [open]);

  const currentSession = sessions.find(s => (s.session_id || s.id) === currentSessionId);
  const currentLabel = currentSession
    ? (currentSession.summary || (currentSessionId ? currentSessionId.slice(0, 8) + '...' : 'Chat'))
    : 'New Chat';

  const filtered = search.trim()
    ? sessions.filter(s => {
        const name = (s.summary || s.session_id || '').toLowerCase();
        return name.includes(search.toLowerCase());
      })
    : sessions;

  const groups = groupSessionsByDate(filtered);

  const handleSelect = useCallback((id) => {
    onSelect(id);
    setOpen(false);
    setSearch('');
  }, [onSelect]);

  const handleNew = useCallback(() => {
    onNew();
    setOpen(false);
    setSearch('');
  }, [onNew]);

  const renderGroup = (label, items) => {
    if (items.length === 0) return null;
    return (
      <div key={label}>
        <div className="px-3 py-1.5 text-[11px] font-medium text-gray-500 uppercase tracking-wider">
          {label}
        </div>
        {items.map((s) => {
          const id = s.session_id || s.id;
          const isSelected = id === currentSessionId;
          const isHovered = id === hoveredId;
          const displayName = s.summary || (id ? id.slice(0, 8) + '...' : 'New Chat');
          return (
            <div
              key={id}
              onMouseEnter={() => setHoveredId(id)}
              onMouseLeave={() => setHoveredId(null)}
              className={`relative px-3 py-2 cursor-pointer transition-colors ${
                isSelected
                  ? 'bg-indigo-600/20 text-gray-200'
                  : 'text-gray-400 hover:bg-gray-800 hover:text-gray-200'
              }`}
              onClick={() => handleSelect(id)}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="text-sm truncate flex-1">{displayName}</span>
                <div className="flex items-center gap-2 shrink-0">
                  <span className="text-[11px] text-gray-600 tabular-nums">
                    {s.message_count || 0}
                  </span>
                  <span className="text-[11px] text-gray-600">
                    {relativeTime(s.last_modified)}
                  </span>
                </div>
              </div>
              {isHovered && onDelete && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onDelete(id);
                  }}
                  className="absolute right-2 top-1/2 -translate-y-1/2 w-5 h-5 flex items-center justify-center text-gray-600 hover:text-red-400 rounded transition-colors"
                  title="Delete session"
                >
                  <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              )}
            </div>
          );
        })}
      </div>
    );
  };

  return (
    <div className="relative" ref={dropdownRef}>
      {/* Trigger button */}
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-2 px-3 py-1.5 rounded-lg hover:bg-gray-800 transition-colors text-sm"
      >
        <svg className="w-4 h-4 text-gray-500" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
        </svg>
        <span className="text-gray-300 truncate max-w-[180px]">{currentLabel}</span>
        <svg className={`w-3 h-3 text-gray-500 transition-transform ${open ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
        </svg>
      </button>

      {/* Dropdown panel */}
      {open && (
        <div className="absolute top-full left-0 mt-1 w-[700px] max-h-[500px] bg-gray-900 border border-gray-700 rounded-xl shadow-2xl shadow-black/40 overflow-hidden z-50 flex flex-col">
          {/* Search + New */}
          <div className="p-2 border-b border-gray-800 flex gap-2">
            <div className="flex-1 relative">
              <svg className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-gray-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
              </svg>
              <input
                ref={searchRef}
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search sessions..."
                className="w-full bg-gray-800/50 border border-gray-700 rounded-lg pl-8 pr-3 py-1.5 text-sm text-gray-300 placeholder-gray-600 focus:outline-none focus:ring-1 focus:ring-indigo-500/50 focus:border-indigo-500/50"
              />
            </div>
            <button
              onClick={handleNew}
              className="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-xs font-medium transition-colors shrink-0"
            >
              New Chat
            </button>
          </div>

          {/* Session list */}
          <div className="flex-1 overflow-y-auto">
            {filtered.length === 0 && (
              <div className="px-4 py-6 text-xs text-gray-600 text-center">
                {search ? 'No matching sessions.' : 'No sessions yet.'}
              </div>
            )}
            {renderGroup('Today', groups.today)}
            {renderGroup('Yesterday', groups.yesterday)}
            {renderGroup('Past week', groups.week)}
            {renderGroup('Older', groups.older)}
          </div>
        </div>
      )}
    </div>
  );
}
