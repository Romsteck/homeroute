import { useState, useCallback, useEffect, useRef, memo } from 'react';

export default memo(function PreviewPanel({ slug, domain, mode = 'full', sendRaw }) {
  const [iframeKey, setIframeKey] = useState(0);
  const [loading, setLoading] = useState(true);
  const [env, setEnv] = useState(() => {
    return localStorage.getItem('studio-preview-env') || 'dev';
  });
  const [currentPath, setCurrentPath] = useState('/');
  const [consoleLogs, setConsoleLogs] = useState([]);
  const [errorCount, setErrorCount] = useState(0);
  const [showConsole, setShowConsole] = useState(false);
  const iframeRef = useRef(null);
  const consoleEndRef = useRef(null);
  const pendingLogsRef = useRef([]);
  const debounceTimerRef = useRef(null);

  const isDev = env === 'dev';
  const currentUrl = isDev
    ? `https://dev.${slug}.${domain}`
    : `https://${slug}.${domain}`;

  const handleRefresh = useCallback(() => {
    setLoading(true);
    setIframeKey(k => k + 1);
  }, []);

  const handleEnvChange = useCallback((newEnv) => {
    setEnv(newEnv);
    setLoading(true);
    setIframeKey(k => k + 1);
    localStorage.setItem('studio-preview-env', newEnv);
    // Reset console state on env switch
    setConsoleLogs([]);
    setErrorCount(0);
    setCurrentPath('/');
  }, []);

  // Reset console state when iframe reloads
  useEffect(() => {
    setConsoleLogs([]);
    setErrorCount(0);
    setCurrentPath('/');
  }, [iframeKey]);

  // postMessage listener for console logs and URL updates
  useEffect(() => {
    const baseDomain = domain;
    function handleMessage(event) {
      // Security: verify origin comes from same base domain
      try {
        const originHost = new URL(event.origin).hostname;
        if (!originHost.endsWith(baseDomain)) return;
      } catch { return; }

      const data = event.data;
      if (!data || !data.type) return;

      if (data.type === '__studio_console') {
        const entry = {
          level: data.level || 'log',
          message: data.message || '',
          timestamp: data.timestamp || Date.now(),
        };
        setConsoleLogs(prev => [...prev, entry]);
        if (entry.level === 'error') {
          setErrorCount(prev => prev + 1);
        }
      } else if (data.type === '__studio_url') {
        setCurrentPath(data.path || '/');
      }
    }

    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, [domain]);

  // Forward console logs to WebSocket (500ms debounce batch)
  useEffect(() => {
    if (consoleLogs.length === 0) return;
    const latest = consoleLogs[consoleLogs.length - 1];
    pendingLogsRef.current.push(latest);

    if (debounceTimerRef.current) clearTimeout(debounceTimerRef.current);
    debounceTimerRef.current = setTimeout(() => {
      if (pendingLogsRef.current.length > 0 && sendRaw) {
        sendRaw({ type: 'console_logs', logs: [...pendingLogsRef.current] });
        pendingLogsRef.current = [];
      }
    }, 500);
  }, [consoleLogs, sendRaw]);

  // Cleanup debounce timer
  useEffect(() => {
    return () => {
      if (debounceTimerRef.current) clearTimeout(debounceTimerRef.current);
    };
  }, []);

  // Auto-scroll console to bottom
  useEffect(() => {
    if (showConsole && consoleEndRef.current) {
      consoleEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [consoleLogs, showConsole]);

  // Reset error badge when console panel opens
  useEffect(() => {
    if (showConsole) setErrorCount(0);
  }, [showConsole]);

  const handleBack = useCallback(() => {
    if (!isDev || !iframeRef.current) return;
    iframeRef.current.contentWindow.postMessage({ type: '__studio_navigate', action: 'back' }, '*');
  }, [isDev]);

  const handleForward = useCallback(() => {
    if (!isDev || !iframeRef.current) return;
    iframeRef.current.contentWindow.postMessage({ type: '__studio_navigate', action: 'forward' }, '*');
  }, [isDev]);

  const handleClearConsole = useCallback(() => {
    setConsoleLogs([]);
    setErrorCount(0);
  }, []);

  const toggleConsole = useCallback(() => {
    setShowConsole(prev => !prev);
  }, []);

  const formatTime = useCallback((ts) => {
    const d = new Date(ts);
    return d.toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }, []);

  // Shared header components
  const envToggle = (
    <div className="flex bg-gray-900 rounded-lg p-0.5">
      <button
        className={`px-2 py-0.5 text-xs rounded-md transition-colors ${
          env === 'dev' ? 'bg-green-600 text-white' : 'text-gray-400 hover:text-gray-200'
        }`}
        onClick={() => handleEnvChange('dev')}
      >
        DEV
      </button>
      <button
        className={`px-2 py-0.5 text-xs rounded-md transition-colors ${
          env === 'prod' ? 'bg-blue-600 text-white' : 'text-gray-400 hover:text-gray-200'
        }`}
        onClick={() => handleEnvChange('prod')}
      >
        PROD
      </button>
    </div>
  );

  const navButtons = (
    <>
      <button
        onClick={handleBack}
        disabled={!isDev}
        className={`p-1 rounded transition-colors ${isDev ? 'text-gray-400 hover:text-gray-200' : 'text-gray-600 cursor-not-allowed'}`}
        title="Back"
      >
        <ChevronLeftIcon className="w-3.5 h-3.5" />
      </button>
      <button
        onClick={handleForward}
        disabled={!isDev}
        className={`p-1 rounded transition-colors ${isDev ? 'text-gray-400 hover:text-gray-200' : 'text-gray-600 cursor-not-allowed'}`}
        title="Forward"
      >
        <ChevronRightIcon className="w-3.5 h-3.5" />
      </button>
    </>
  );

  const urlDisplay = (
    <span className={`text-xs font-mono truncate flex-1 ${isDev ? 'text-gray-400' : 'text-gray-600'}`}>
      {isDev ? currentPath : currentUrl}
    </span>
  );

  const consoleButton = (
    <button
      onClick={toggleConsole}
      className={`p-1 relative rounded transition-colors ${showConsole ? 'text-green-400 bg-gray-700' : 'text-gray-500 hover:text-gray-300'}`}
      title="Console"
    >
      <ConsoleIcon className="w-3.5 h-3.5" />
      {errorCount > 0 && !showConsole && (
        <span className="absolute -top-1 -right-1 bg-red-500 text-white text-[9px] font-bold rounded-full min-w-[14px] h-[14px] flex items-center justify-center px-0.5">
          {errorCount > 99 ? '99+' : errorCount}
        </span>
      )}
    </button>
  );

  const consolePanel = showConsole && (
    <div className="h-52 flex flex-col bg-gray-950 border-t border-gray-700 shrink-0">
      <div className="h-7 flex items-center justify-between px-3 border-b border-gray-800 shrink-0">
        <span className="text-xs text-gray-400 font-medium">Console</span>
        <div className="flex items-center gap-1">
          <button
            onClick={handleClearConsole}
            className="p-0.5 text-gray-500 hover:text-gray-300 rounded transition-colors"
            title="Clear console"
          >
            <TrashIcon className="w-3 h-3" />
          </button>
          <button
            onClick={toggleConsole}
            className="p-0.5 text-gray-500 hover:text-gray-300 rounded transition-colors"
            title="Close console"
          >
            <CloseIcon className="w-3 h-3" />
          </button>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto overflow-x-hidden px-2 py-1 text-xs font-mono">
        {consoleLogs.length === 0 && (
          <div className="text-gray-600 text-center py-4">No console output</div>
        )}
        {consoleLogs.map((log, i) => (
          <div key={i} className="flex items-start gap-1.5 py-0.5 hover:bg-gray-900/50">
            <LogLevelIcon level={log.level} />
            <span className={`flex-1 break-all ${logColor(log.level)}`}>
              {log.message}
            </span>
            <span className="text-gray-600 whitespace-nowrap shrink-0 ml-2">
              {formatTime(log.timestamp)}
            </span>
          </div>
        ))}
        <div ref={consoleEndRef} />
      </div>
    </div>
  );

  const iframeEl = (
    <iframe
      ref={iframeRef}
      key={iframeKey}
      src={currentUrl}
      className="absolute inset-0 w-full h-full border-0"
      sandbox="allow-same-origin allow-scripts allow-forms allow-popups allow-modals allow-downloads"
      title={`Preview: ${currentUrl}`}
      onLoad={() => setLoading(false)}
      onError={() => setLoading(false)}
    />
  );

  if (mode === 'split') {
    return (
      <div className="flex-1 flex flex-col min-h-0 bg-gray-900">
        {/* Header */}
        <div className="h-8 flex items-center gap-1.5 px-2 bg-gray-800/50 border-b border-gray-700 shrink-0">
          {envToggle}
          {navButtons}
          {urlDisplay}
          <button
            onClick={handleRefresh}
            className="p-1 text-gray-500 hover:text-gray-300 rounded transition-colors"
            title="Refresh preview"
          >
            <RefreshIcon className="w-3.5 h-3.5" />
          </button>
          {consoleButton}
        </div>
        {/* iframe */}
        <div className="flex-1 relative bg-white">
          {loading && <LoadingOverlay />}
          {iframeEl}
        </div>
        {/* Console panel */}
        {consolePanel}
      </div>
    );
  }

  // Full mode
  return (
    <div className="flex-1 flex flex-col min-h-0 bg-gray-900">
      {/* Header bar */}
      <div className="h-10 bg-gray-800 border-b border-gray-700 flex items-center px-3 gap-2 shrink-0">
        {envToggle}
        {navButtons}
        {urlDisplay}
        {/* Refresh */}
        <button
          onClick={handleRefresh}
          className="p-1.5 text-gray-500 hover:text-gray-300 rounded transition-colors"
          title="Refresh"
        >
          <RefreshIcon className="w-4 h-4" />
        </button>
        {/* Console */}
        {consoleButton}
        {/* Open in new tab */}
        <a
          href={currentUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="p-1.5 text-gray-500 hover:text-gray-300 rounded transition-colors"
          title="Open in new tab"
        >
          <ExternalLinkIcon className="w-4 h-4" />
        </a>
      </div>
      {/* iframe */}
      <div className="flex-1 relative bg-white">
        {loading && <LoadingOverlay />}
        {iframeEl}
      </div>
      {/* Console panel */}
      {consolePanel}
    </div>
  );
});

function logColor(level) {
  switch (level) {
    case 'error': return 'text-red-400';
    case 'warn': return 'text-yellow-400';
    case 'info': return 'text-blue-400';
    default: return 'text-gray-400';
  }
}

function LogLevelIcon({ level }) {
  switch (level) {
    case 'error':
      return <span className="w-3 h-3 mt-0.5 shrink-0 rounded-full bg-red-500/20 flex items-center justify-center"><span className="w-1.5 h-1.5 rounded-full bg-red-400" /></span>;
    case 'warn':
      return (
        <span className="w-3 h-3 mt-0.5 shrink-0 flex items-center justify-center text-yellow-400">
          <svg viewBox="0 0 12 12" fill="currentColor" className="w-2.5 h-2.5"><path d="M6 0L12 11H0z" /></svg>
        </span>
      );
    case 'info':
      return <span className="w-3 h-3 mt-0.5 shrink-0 rounded-full bg-blue-500/20 flex items-center justify-center text-blue-400 text-[8px] font-bold">i</span>;
    default:
      return <span className="w-3 h-3 mt-0.5 shrink-0 rounded-full bg-gray-500/20 flex items-center justify-center"><span className="w-1.5 h-1.5 rounded-full bg-gray-500" /></span>;
  }
}

function LoadingOverlay() {
  return (
    <div className="absolute inset-0 bg-gray-900 flex items-center justify-center z-10">
      <div className="flex flex-col items-center gap-3">
        <svg className="w-8 h-8 text-blue-500 animate-spin" fill="none" viewBox="0 0 24 24">
          <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
          <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>
        </svg>
        <span className="text-sm text-gray-400">Loading preview...</span>
      </div>
    </div>
  );
}

function RefreshIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
    </svg>
  );
}

function ExternalLinkIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" />
    </svg>
  );
}

function ChevronLeftIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 19.5L8.25 12l7.5-7.5" />
    </svg>
  );
}

function ChevronRightIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M8.25 4.5l7.5 7.5-7.5 7.5" />
    </svg>
  );
}

function ConsoleIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M6.75 7.5l3 2.25-3 2.25m4.5 0h3M5.25 20.25h13.5A2.25 2.25 0 0021 18V6a2.25 2.25 0 00-2.25-2.25H5.25A2.25 2.25 0 003 6v12a2.25 2.25 0 002.25 2.25z" />
    </svg>
  );
}

function TrashIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
    </svg>
  );
}

function CloseIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
    </svg>
  );
}
