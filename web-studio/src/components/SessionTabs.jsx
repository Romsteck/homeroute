import { useState, useRef, useEffect } from 'react';

function TypeIcon({ sessionType, className = '' }) {
  if (sessionType === 'cli') {
    // Terminal icon >_
    return (
      <span className={`text-emerald-400 font-mono text-[10px] leading-none font-bold shrink-0 ${className}`}>
        {'>_'}
      </span>
    );
  }
  // Agent chat bubble icon
  return (
    <svg className={`w-3 h-3 text-indigo-400 shrink-0 ${className}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
    </svg>
  );
}

export default function SessionTabs({ tabs, activeIndex, onSwitch, onClose, onNew }) {
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const dropdownRef = useRef(null);

  // Close dropdown on click outside
  useEffect(() => {
    if (!dropdownOpen) return;
    const handler = (e) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target)) {
        setDropdownOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [dropdownOpen]);

  return (
    <div className="h-9 bg-gray-900 border-b border-gray-800 flex items-center px-2 gap-0.5 shrink-0">
      {/* Scrollable tabs area */}
      <div className="flex-1 flex items-center gap-0.5 overflow-x-auto min-w-0">
        {tabs.map((tab, index) => {
          const isActive = index === activeIndex;
          return (
            <button
              key={tab.id || `new-${index}`}
              onClick={() => onSwitch(index)}
              className={`group relative flex items-center gap-1.5 px-3 h-7 rounded-md text-xs font-medium transition-colors max-w-[180px] shrink-0 ${
                isActive
                  ? 'bg-gray-800 text-gray-200 border-b-2 border-indigo-500'
                  : 'text-gray-500 hover:text-gray-300 hover:bg-gray-800/50'
              }`}
            >
              {/* Session type icon */}
              <TypeIcon sessionType={tab.sessionType} />
              {/* Streaming indicator (background tab) */}
              {tab.isStreaming && !isActive && (
                <span className="w-1.5 h-1.5 bg-purple-500 rounded-full animate-pulse shrink-0" />
              )}
              {/* Unread indicator */}
              {tab.hasUnread && !isActive && !tab.isStreaming && (
                <span className="w-1.5 h-1.5 bg-indigo-400 rounded-full shrink-0" />
              )}
              <span className="truncate">{tab.label}</span>
              {/* Close button */}
              {tabs.length > 1 && (
                <span
                  onClick={(e) => {
                    e.stopPropagation();
                    onClose(index);
                  }}
                  className="ml-0.5 w-4 h-4 flex items-center justify-center rounded text-gray-600 hover:text-gray-300 hover:bg-gray-700 opacity-0 group-hover:opacity-100 transition-opacity shrink-0"
                >
                  <svg className="w-2.5 h-2.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </span>
              )}
            </button>
          );
        })}
      </div>
      {/* New tab dropdown — outside scrollable area so it's never clipped */}
      <div className="relative shrink-0" ref={dropdownRef}>
        <button
          onClick={() => setDropdownOpen(!dropdownOpen)}
          className="flex items-center justify-center w-7 h-7 rounded-md text-gray-600 hover:text-gray-300 hover:bg-gray-800/50 transition-colors"
          title="New tab"
        >
          <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
          </svg>
        </button>
        {dropdownOpen && (
          <div className="absolute top-full right-0 mt-1 w-36 bg-gray-800 border border-gray-700 rounded-lg shadow-xl shadow-black/40 overflow-hidden z-50">
            <button
              onClick={() => {
                onNew('agent');
                setDropdownOpen(false);
              }}
              className="w-full flex items-center gap-2 px-3 py-2 text-xs text-gray-300 hover:bg-gray-700 transition-colors"
            >
              <svg className="w-3.5 h-3.5 text-indigo-400" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
              </svg>
              Agent
            </button>
            <button
              onClick={() => {
                onNew('cli');
                setDropdownOpen(false);
              }}
              className="w-full flex items-center gap-2 px-3 py-2 text-xs text-gray-300 hover:bg-gray-700 transition-colors"
            >
              <span className="text-emerald-400 font-mono text-[11px] font-bold w-3.5 text-center">{'>_'}</span>
              Terminal
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
