import { useState, useCallback } from 'react';

export default function PreviewPanel({ slug, domain }) {
  const [iframeKey, setIframeKey] = useState(0);
  const [loading, setLoading] = useState(true);
  const previewUrl = `https://dev.${slug}.${domain}`;

  const handleRefresh = useCallback(() => {
    setLoading(true);
    setIframeKey(k => k + 1);
  }, []);

  const handleOpenExternal = useCallback(() => {
    window.open(previewUrl, '_blank');
  }, [previewUrl]);

  return (
    <div className="flex-1 flex flex-col min-h-0 bg-gray-900">
      {/* Toolbar */}
      <div className="h-10 flex items-center justify-between px-4 bg-gray-800 border-b border-gray-700 shrink-0">
        <div className="flex items-center gap-2">
          <svg className="w-4 h-4 text-green-400" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z" />
            <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
          </svg>
          <span className="text-sm font-medium text-gray-200">Preview</span>
          <span className="text-xs text-gray-500 ml-2">dev.{slug}.{domain}</span>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={handleRefresh}
            className="p-1.5 text-gray-400 hover:text-gray-200 hover:bg-gray-700 rounded transition-colors"
            title="Refresh preview"
          >
            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
            </svg>
          </button>
          <button
            onClick={handleOpenExternal}
            className="p-1.5 text-gray-400 hover:text-gray-200 hover:bg-gray-700 rounded transition-colors"
            title="Open in new tab"
          >
            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" />
            </svg>
          </button>
        </div>
      </div>

      {/* iframe */}
      <div className="flex-1 relative bg-white">
        {loading && (
          <div className="absolute inset-0 bg-gray-900 flex items-center justify-center z-10">
            <div className="flex flex-col items-center gap-3">
              <svg className="w-8 h-8 text-blue-500 animate-spin" fill="none" viewBox="0 0 24 24">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>
              </svg>
              <span className="text-sm text-gray-400">Loading preview...</span>
            </div>
          </div>
        )}
        <iframe
          key={iframeKey}
          src={previewUrl}
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
