import { useEffect, useRef } from 'react';

export default function ActivityPanel({ activities, onClose }) {
  const scrollRef = useRef(null);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [activities]);

  return (
    <div className="w-[300px] bg-gray-800 border-l border-gray-700 flex flex-col shrink-0">
      {/* Header */}
      <div className="h-10 flex items-center justify-between px-3 border-b border-gray-700 shrink-0">
        <div className="flex items-center gap-2">
          <span className="text-amber-500">
            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z" />
            </svg>
          </span>
          <span className="text-sm font-medium text-gray-200">Activity</span>
          <span className="text-xs text-gray-500">{activities.length}</span>
        </div>
        <button
          onClick={onClose}
          className="w-6 h-6 flex items-center justify-center text-gray-500 hover:text-gray-300 hover:bg-gray-700 rounded transition-colors"
          title="Close activity panel"
        >
          <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>

      {/* Activity list */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto">
        {activities.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-gray-600 px-4">
            <svg className="w-10 h-10 mb-3 opacity-50" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z" />
            </svg>
            <span className="text-xs text-center">Tool activities will appear here during agent execution.</span>
          </div>
        ) : (
          activities.map((act) => (
            <ActivityEntry key={act.id} activity={act} />
          ))
        )}
      </div>
    </div>
  );
}

function ActivityEntry({ activity }) {
  const { tool, description, status } = activity;

  return (
    <div className="px-3 py-2 border-b border-gray-700/50 hover:bg-gray-700/30 transition-colors">
      <div className="flex items-center gap-2">
        <StatusDot status={status} />
        <span className="text-sm font-medium text-purple-400 truncate">{tool}</span>
      </div>
      {description && (
        <div className="mt-1 ml-5 text-xs text-gray-500 truncate font-mono" title={description}>
          {description}
        </div>
      )}
    </div>
  );
}

function StatusDot({ status }) {
  if (status === 'running') {
    return <span className="inline-block w-2.5 h-2.5 bg-amber-500 rounded-full animate-pulse shrink-0"></span>;
  }
  if (status === 'error') {
    return (
      <span className="inline-flex items-center justify-center w-2.5 h-2.5 shrink-0">
        <svg className="w-3 h-3 text-red-500" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
        </svg>
      </span>
    );
  }
  // done
  return (
    <span className="inline-flex items-center justify-center w-2.5 h-2.5 shrink-0">
      <svg className="w-3 h-3 text-green-500" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
        <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 12.75l6 6 9-13.5" />
      </svg>
    </span>
  );
}
