import { useState, useRef, useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';

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
  const [choosingId, setChoosingId] = useState(null);
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
    setChoosingId(choosingId === id ? null : id);
  }, [choosingId]);

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
          const isChoosing = choosingId === id;
          return (
            <div
              key={id}
              onMouseEnter={() => setHoveredId(id)}
              onMouseLeave={() => setHoveredId(null)}
              className={`group relative px-3 py-2 cursor-pointer transition-colors ${
                isSelected
                  ? 'bg-indigo-600/20 text-gray-200'
                  : 'text-gray-400 hover:bg-gray-800 hover:text-gray-200'
              }`}
              onClick={() => handleSelect(id)}
            >
              <div className="flex items-center justify-between gap-2">
                {s.session_type === 'cli' ? (
                  <span className="text-emerald-400 font-mono text-[10px] leading-none font-bold shrink-0">{'>_'}</span>
                ) : (
                  <svg className="w-3 h-3 text-indigo-400 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
                  </svg>
                )}
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
              {/* Open mode choice buttons */}
              {isChoosing && (
                <div className="flex gap-2 mt-1.5 ml-5" onClick={(e) => e.stopPropagation()}>
                  <button
                    onClick={() => { onSelect(id, displayName, 'agent'); setOpen(false); setSearch(''); setChoosingId(null); }}
                    className="flex items-center gap-1.5 px-2.5 py-1 rounded bg-indigo-600/20 text-indigo-400 text-xs hover:bg-indigo-600/30 transition-colors"
                  >
                    <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
                    </svg>
                    Open in Agent
                  </button>
                  <button
                    onClick={() => { onSelect(id, displayName, 'cli'); setOpen(false); setSearch(''); setChoosingId(null); }}
                    className="flex items-center gap-1.5 px-2.5 py-1 rounded bg-emerald-600/20 text-emerald-400 text-xs hover:bg-emerald-600/30 transition-colors"
                  >
                    <span className="font-mono text-[10px] font-bold">{'>_'}</span>
                    Resume in Terminal
                  </button>
                </div>
              )}
              {onDelete && !isChoosing && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    e.preventDefault();
                    onDelete(id);
                  }}
                  className={`absolute right-2 top-1/2 -translate-y-1/2 w-6 h-6 flex items-center justify-center rounded transition-all ${
                    isHovered ? 'text-red-400 bg-red-900/30 opacity-100' : 'text-gray-700 opacity-0 group-hover:opacity-100'
                  } hover:text-red-300 hover:bg-red-900/50`}
                  title="Delete session"
                >
                  <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
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

      {/* Portal: backdrop + dropdown rendered in document.body to escape stacking contexts */}
      {open && createPortal(
        <>
          <div className="fixed inset-0 bg-black/40" style={{zIndex: 99998}} onClick={() => setOpen(false)} />
          <div
            className="fixed w-[700px] max-h-[500px] bg-gray-900 border border-gray-700 rounded-xl shadow-2xl shadow-black/40 overflow-hidden flex flex-col"
            style={{
              zIndex: 99999,
              top: (dropdownRef.current?.getBoundingClientRect().bottom ?? 48) + 4,
              left: dropdownRef.current?.getBoundingClientRect().left ?? 0,
            }}
            onClick={(e) => e.stopPropagation()}
            onMouseDown={(e) => e.stopPropagation()}
          >
          {/* Search */}
          <div className="p-2 border-b border-gray-800">
            <div className="relative">
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
        </>,
        document.body
      )}
    </div>
  );
}
