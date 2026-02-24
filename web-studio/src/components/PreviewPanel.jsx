import { useState, useCallback } from 'react';

export default function PreviewPanel({ slug, domain, mode = 'full' }) {
  const [iframeKey, setIframeKey] = useState(0);
  const [loading, setLoading] = useState(true);
  const [env, setEnv] = useState(() => {
    if (mode === 'full') {
      return localStorage.getItem('studio-preview-env') || 'dev';
    }
    return 'dev';
  });

  const currentUrl = env === 'dev'
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
  }, []);

  if (mode === 'split') {
    return (
      <div className="flex-1 flex flex-col min-h-0 bg-gray-900">
        {/* Minimal header */}
        <div className="h-8 flex items-center justify-between px-3 bg-gray-800/50 border-b border-gray-700 shrink-0">
          <span className="text-xs text-gray-500 font-mono truncate">dev.{slug}.{domain}</span>
          <button
            onClick={handleRefresh}
            className="p-1 text-gray-500 hover:text-gray-300 rounded transition-colors"
            title="Refresh preview"
          >
            <RefreshIcon className="w-3.5 h-3.5" />
          </button>
        </div>
        {/* iframe */}
        <div className="flex-1 relative bg-white">
          {loading && <LoadingOverlay />}
          <iframe
            key={iframeKey}
            src={currentUrl}
            className="absolute inset-0 w-full h-full border-0"
            sandbox="allow-same-origin allow-scripts allow-forms allow-popups allow-modals allow-downloads"
            title={`Preview: dev.${slug}.${domain}`}
            onLoad={() => setLoading(false)}
            onError={() => setLoading(false)}
          />
        </div>
      </div>
    );
  }

  // Full mode
  return (
    <div className="flex-1 flex flex-col min-h-0 bg-gray-900">
      {/* Header bar */}
      <div className="h-10 bg-gray-800 border-b border-gray-700 flex items-center px-3 gap-3 shrink-0">
        {/* DEV/PROD toggle */}
        <div className="flex bg-gray-900 rounded-lg p-0.5">
          <button
            className={`px-3 py-1 text-xs rounded-md transition-colors ${
              env === 'dev' ? 'bg-green-600 text-white' : 'text-gray-400 hover:text-gray-200'
            }`}
            onClick={() => handleEnvChange('dev')}
          >
            DEV
          </button>
          <button
            className={`px-3 py-1 text-xs rounded-md transition-colors ${
              env === 'prod' ? 'bg-blue-600 text-white' : 'text-gray-400 hover:text-gray-200'
            }`}
            onClick={() => handleEnvChange('prod')}
          >
            PROD
          </button>
        </div>
        {/* URL display */}
        <span className="text-xs text-gray-500 font-mono truncate flex-1">{currentUrl}</span>
        {/* Refresh */}
        <button
          onClick={handleRefresh}
          className="p-1.5 text-gray-500 hover:text-gray-300 rounded transition-colors"
          title="Refresh"
        >
          <RefreshIcon className="w-4 h-4" />
        </button>
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
        <iframe
          key={iframeKey}
          src={currentUrl}
          className="absolute inset-0 w-full h-full border-0"
          sandbox="allow-same-origin allow-scripts allow-forms allow-popups allow-modals allow-downloads"
          title={`Preview: ${currentUrl}`}
          onLoad={() => setLoading(false)}
          onError={() => setLoading(false)}
        />
      </div>
    </div>
  );
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
