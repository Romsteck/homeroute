import { useState, memo } from 'react';

export default memo(function CodeServerPanel({ slug, domain }) {
  const [loading, setLoading] = useState(true);

  const codeServerUrl = `https://code.${slug}.${domain}/?folder=/root/workspace`;

  return (
    <div className="flex-1 flex flex-col min-h-0 bg-gray-900">
      <div className="flex-1 relative bg-gray-950">
        {loading && (
          <div className="absolute inset-0 bg-gray-900 flex items-center justify-center z-10">
            <div className="flex flex-col items-center gap-3">
              <svg className="w-8 h-8 text-blue-500 animate-spin" fill="none" viewBox="0 0 24 24">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
              <span className="text-sm text-gray-400">Loading Code Server...</span>
            </div>
          </div>
        )}
        <iframe
          src={codeServerUrl}
          className="absolute inset-0 w-full h-full border-0"
          allow="clipboard-read; clipboard-write"
          title="Code Server"
          onLoad={() => setLoading(false)}
          onError={() => setLoading(false)}
        />
      </div>
    </div>
  );
});
