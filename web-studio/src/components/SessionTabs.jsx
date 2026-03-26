import SessionPicker from './SessionPicker';

export default function SessionTabs({
  tabs,
  activeIndex,
  onSwitch,
  onClose,
  onNew,
  sessions,
  currentSessionId,
  onSelectSession,
  onDeleteSession,
}) {
  return (
    <div className="h-9 bg-gray-900 border-b border-gray-800 flex items-center px-2 gap-0.5 shrink-0">
      {/* New button */}
      <button
        onClick={() => onNew('agent')}
        className="flex items-center gap-1 px-2 h-7 rounded-md bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium transition-colors shrink-0 mr-0.5"
        title="New session"
      >
        <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
        </svg>
        New
      </button>
      {/* Session picker */}
      <SessionPicker
        sessions={sessions}
        currentSessionId={currentSessionId}
        onSelect={onSelectSession}
        onNew={onNew}
        onDelete={onDeleteSession}
      />
      <div className="h-5 w-px bg-gray-800 mx-1 shrink-0" />
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
              {/* Agent icon */}
              <svg className="w-3 h-3 text-indigo-400 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
              </svg>
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
            </button>
          );
        })}
      </div>
    </div>
  );
}
