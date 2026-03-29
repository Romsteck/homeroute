import { useState, useRef, useEffect, useCallback } from 'react';

export default function AppStatusDropdown({ apps, onStart, onStop, onRestart, onStartAll, onStopAll }) {
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState({});
  const ref = useRef(null);

  const runningCount = apps.filter(a => a.status === 'running').length;
  const totalCount = apps.length;
  const hasRunning = runningCount > 0;

  // Close on click outside
  useEffect(() => {
    if (!open) return;
    function handleClick(e) {
      if (ref.current && !ref.current.contains(e.target)) setOpen(false);
    }
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [open]);

  const withPending = useCallback((slug, action, fn) => {
    setPending(p => ({ ...p, [slug]: action }));
    Promise.resolve(fn(slug)).finally(() => {
      setTimeout(() => setPending(p => {
        const next = { ...p };
        delete next[slug];
        return next;
      }), 2000);
    });
  }, []);

  return (
    <div className="relative" ref={ref}>
      <button
        onClick={() => setOpen(o => !o)}
        className="flex items-center gap-1.5 px-2 py-1 rounded-md text-xs text-gray-400 hover:text-gray-200 hover:bg-gray-800/50 transition-colors"
      >
        <span className={`inline-block w-1.5 h-1.5 rounded-full ${hasRunning ? 'bg-green-500' : 'bg-gray-600'}`} />
        <span>{runningCount}/{totalCount} running</span>
        <svg className={`w-3 h-3 transition-transform ${open ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 8.25l-7.5 7.5-7.5-7.5" />
        </svg>
      </button>

      {open && (
        <div className="absolute top-full right-0 mt-1 w-72 bg-gray-900 border border-gray-700 rounded-lg shadow-xl z-50 overflow-hidden">
          <div className="px-3 py-2 border-b border-gray-800">
            <span className="text-xs font-semibold text-gray-300 uppercase tracking-wider">Applications</span>
          </div>

          <div className="max-h-64 overflow-y-auto">
            {apps.length === 0 && (
              <div className="px-3 py-4 text-xs text-gray-500 text-center">No applications found</div>
            )}
            {apps.map(app => {
              const isRunning = app.status === 'running';
              const isFailed = app.status === 'failed' || app.status === 'error';
              const isPending = !!pending[app.slug];
              const dotColor = isRunning ? 'bg-green-500' : isFailed ? 'bg-red-500' : 'bg-gray-600';

              return (
                <div key={app.slug} className="flex items-center justify-between px-3 py-1.5 hover:bg-gray-800/50">
                  <div className="flex items-center gap-2 min-w-0">
                    <span className={`inline-block w-1.5 h-1.5 rounded-full shrink-0 ${dotColor}`} />
                    <span className="text-sm text-gray-300 truncate">{app.name || app.slug}</span>
                    {app.port && (
                      <span className="text-[10px] text-gray-600">:{app.port}</span>
                    )}
                  </div>
                  <div className="flex items-center gap-1 shrink-0 ml-2">
                    {isPending ? (
                      <Spinner />
                    ) : isRunning ? (
                      <>
                        <ActionBtn
                          title="Restart"
                          onClick={() => withPending(app.slug, 'restart', onRestart)}
                          className="text-blue-400 hover:bg-blue-500/15"
                        >
                          <path strokeLinecap="round" strokeLinejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
                        </ActionBtn>
                        <ActionBtn
                          title="Stop"
                          onClick={() => withPending(app.slug, 'stop', onStop)}
                          className="text-red-400 hover:bg-red-500/15"
                        >
                          <rect x="6" y="6" width="12" height="12" rx="1" strokeLinecap="round" strokeLinejoin="round" />
                        </ActionBtn>
                      </>
                    ) : (
                      <ActionBtn
                        title="Start"
                        onClick={() => withPending(app.slug, 'start', onStart)}
                        className="text-green-400 hover:bg-green-500/15"
                      >
                        <path strokeLinecap="round" strokeLinejoin="round" d="M5.25 5.653c0-.856.917-1.398 1.667-.986l11.54 6.348a1.125 1.125 0 010 1.971l-11.54 6.347a1.125 1.125 0 01-1.667-.985V5.653z" />
                      </ActionBtn>
                    )}
                  </div>
                </div>
              );
            })}
          </div>

          <div className="flex items-center gap-2 px-3 py-2 border-t border-gray-800">
            <button
              onClick={onStartAll}
              className="flex-1 text-xs py-1 rounded bg-green-600/15 text-green-400 hover:bg-green-600/25 transition-colors"
            >
              Start All
            </button>
            <button
              onClick={onStopAll}
              className="flex-1 text-xs py-1 rounded bg-red-600/15 text-red-400 hover:bg-red-600/25 transition-colors"
            >
              Stop All
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function ActionBtn({ title, onClick, className, children }) {
  return (
    <button
      title={title}
      onClick={onClick}
      className={`p-1 rounded transition-colors ${className}`}
    >
      <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
        {children}
      </svg>
    </button>
  );
}

function Spinner() {
  return (
    <svg className="w-3.5 h-3.5 animate-spin text-gray-500" fill="none" viewBox="0 0 24 24">
      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
    </svg>
  );
}
