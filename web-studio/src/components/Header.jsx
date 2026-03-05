import { useState, useRef, useEffect } from 'react';
import { createPortal } from 'react-dom';
import SessionPicker from './SessionPicker';

export default function Header({
  appName,
  activeTab,
  onTabChange,
  connected,
  sessions,
  currentSessionId,
  onSelectSession,
  onNewSession,
  onDeleteSession,
  authStatus,
  onOpenAuthDialog,
}) {
  const tabs = [
    { id: 'studio', label: 'Studio', icon: StudioIcon },
    { id: 'preview', label: 'Preview', icon: PreviewIcon },
    { id: 'files', label: 'Files', icon: FilesIcon },
    { id: 'docs', label: 'Docs', icon: DocsIcon },
  ];

  const [newMenuOpen, setNewMenuOpen] = useState(false);
  const newMenuRef = useRef(null);

  useEffect(() => {
    if (!newMenuOpen) return;
    const handler = (e) => {
      if (newMenuRef.current && !newMenuRef.current.contains(e.target)) {
        // Also check if click is inside the portal dropdown
        const portal = document.getElementById('new-menu-portal');
        if (portal && portal.contains(e.target)) return;
        setNewMenuOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [newMenuOpen]);

  return (
    <header className="h-12 bg-gray-900/80 backdrop-blur border-b border-gray-800 flex items-center justify-between px-4 shrink-0">
      {/* Left: New button + Session picker + title */}
      <div className="flex items-center gap-2">
        <div className="relative" ref={newMenuRef}>
          <button
            onClick={() => setNewMenuOpen(!newMenuOpen)}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium transition-colors"
            title="New session"
          >
            <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
            </svg>
            New
            <svg className={`w-3 h-3 transition-transform ${newMenuOpen ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
            </svg>
          </button>
          {newMenuOpen && createPortal(
            <div
              id="new-menu-portal"
              className="fixed w-40 bg-gray-800 border border-gray-700 rounded-lg shadow-xl shadow-black/40 overflow-hidden"
              style={{
                zIndex: 99999,
                top: (newMenuRef.current?.getBoundingClientRect().bottom ?? 48) + 4,
                left: newMenuRef.current?.getBoundingClientRect().left ?? 0,
              }}
            >
              <button
                onClick={() => { onNewSession('agent'); setNewMenuOpen(false); }}
                className="w-full flex items-center gap-2.5 px-3 py-2 text-xs text-gray-300 hover:bg-gray-700 transition-colors"
              >
                <svg className="w-3.5 h-3.5 text-indigo-400" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
                </svg>
                Agent
              </button>
              <button
                onClick={() => { onNewSession('cli'); setNewMenuOpen(false); }}
                className="w-full flex items-center gap-2.5 px-3 py-2 text-xs text-gray-300 hover:bg-gray-700 transition-colors"
              >
                <span className="text-emerald-400 font-mono text-[11px] font-bold w-3.5 text-center">{'>_'}</span>
                Terminal
              </button>
            </div>,
            document.body
          )}
        </div>
        <SessionPicker
          sessions={sessions}
          currentSessionId={currentSessionId}
          onSelect={(id, label, sessionType) => {
            const session = sessions.find(s => (s.session_id || s.id) === id);
            const displayLabel = label || session?.summary || (id ? id.slice(0, 8) + '...' : 'New Chat');
            onSelectSession(id, displayLabel, sessionType);
          }}
          onNew={onNewSession}
          onDelete={onDeleteSession}
        />
        <div className="h-5 w-px bg-gray-800" />
        <span className="text-sm text-gray-400 font-medium">
          Studio <span className="text-gray-600">&mdash;</span> <span className="text-gray-300">{appName}</span>
        </span>
      </div>

      {/* Center: tabs */}
      <div className="flex items-center gap-1">
        {tabs.map((tab) => {
          const isActive = activeTab === tab.id;
          const Icon = tab.icon;
          return (
            <button
              key={tab.id}
              onClick={() => onTabChange(tab.id)}
              className={`px-3 py-1.5 rounded-lg text-sm font-medium transition-colors flex items-center gap-2 ${
                isActive
                  ? 'bg-indigo-600/15 text-indigo-400'
                  : 'text-gray-500 hover:text-gray-300 hover:bg-gray-800/50'
              }`}
            >
              <Icon className="w-4 h-4" />
              {tab.label}
            </button>
          );
        })}
      </div>

      {/* Right: auth + connection status */}
      <div className="flex items-center gap-3">
        {authStatus?.authenticated ? (
          <button
            onClick={onOpenAuthDialog}
            className="flex items-center gap-1.5 px-2 py-1 rounded-md text-xs text-green-400 hover:bg-green-500/10 transition-colors"
          >
            <span className="inline-block w-1.5 h-1.5 rounded-full bg-green-500" />
            Claude Linked
          </button>
        ) : (
          <button
            onClick={onOpenAuthDialog}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium text-amber-400 bg-amber-500/10 hover:bg-amber-500/20 transition-colors"
          >
            Link Claude Account
          </button>
        )}
        <div className="h-4 w-px bg-gray-800" />
        <div className="flex items-center gap-2">
          <span className={`inline-block w-2 h-2 rounded-full ${connected ? 'bg-green-500' : 'bg-red-500'}`} />
          <span className={`text-xs ${connected ? 'text-gray-500' : 'text-red-400'}`}>
            {connected ? 'Connected' : 'Disconnected'}
          </span>
        </div>
      </div>
    </header>
  );
}

function StudioIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M6.75 7.5l3 2.25-3 2.25m4.5 0h3M5.25 20.25h13.5A2.25 2.25 0 0021 18V6a2.25 2.25 0 00-2.25-2.25H5.25A2.25 2.25 0 003 6v12a2.25 2.25 0 002.25 2.25z" />
    </svg>
  );
}

function PreviewIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z" />
      <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
    </svg>
  );
}

function FilesIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
    </svg>
  );
}

function DocsIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M12 6.042A8.967 8.967 0 006 3.75c-1.052 0-2.062.18-3 .512v14.25A8.987 8.987 0 016 18c2.305 0 4.408.867 6 2.292m0-14.25a8.966 8.966 0 016-2.292c1.052 0 2.062.18 3 .512v14.25A8.987 8.987 0 0018 18a8.967 8.967 0 00-6 2.292m0-14.25v14.25" />
    </svg>
  );
}
