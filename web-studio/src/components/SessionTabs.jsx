export default function SessionTabs({ tabs, activeIndex, onSwitch, onClose, onNew }) {
  return (
    <div className="h-9 bg-gray-900 border-b border-gray-800 flex items-center px-2 gap-0.5 overflow-x-auto shrink-0">
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
      {/* New tab button */}
      <button
        onClick={onNew}
        className="flex items-center justify-center w-7 h-7 rounded-md text-gray-600 hover:text-gray-300 hover:bg-gray-800/50 transition-colors shrink-0"
        title="New tab"
      >
        <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
        </svg>
      </button>
    </div>
  );
}
