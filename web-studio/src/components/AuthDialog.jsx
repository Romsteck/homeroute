import { useState, useCallback, useEffect } from 'react';

export default function AuthDialog({ authStatus, authEvent, onClose, sendRaw, onUnlink }) {
  const [token, setToken] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);

  // Handle auth events from WS
  useEffect(() => {
    if (!authEvent) return;
    if (authEvent.type === 'oauth_error') {
      setLoading(false);
      setError(authEvent.message || 'Authentication error');
    } else if (authEvent.type === 'oauth_success') {
      setLoading(false);
    }
  }, [authEvent]);

  const handleSubmitToken = useCallback(() => {
    const trimmed = token.trim();
    if (!trimmed || !sendRaw) return;
    setLoading(true);
    setError(null);
    sendRaw({ type: 'submit_token', token: trimmed });
  }, [token, sendRaw]);

  const handleUnlink = useCallback(() => {
    if (onUnlink) onUnlink();
  }, [onUnlink]);

  const isAuthenticated = authStatus?.authenticated === true;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="bg-gray-900 border border-gray-700 rounded-xl shadow-2xl shadow-black/50 w-full max-w-md mx-4"
        onClick={e => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-gray-800">
          <h2 className="text-sm font-semibold text-gray-200">Link Claude Account</h2>
          <button onClick={onClose} className="text-gray-500 hover:text-gray-300 transition-colors">
            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Body */}
        <div className="px-5 py-4">
          {/* Already authenticated */}
          {isAuthenticated && (
            <div className="space-y-4">
              <div className="flex items-center gap-2">
                <span className="inline-block w-2 h-2 rounded-full bg-green-500" />
                <span className="text-sm text-gray-300">Account linked</span>
                {authStatus.method && (
                  <span className="text-xs text-gray-600">({authStatus.method})</span>
                )}
              </div>
              <button
                onClick={handleUnlink}
                className="w-full px-3 py-2 rounded-lg text-sm font-medium text-red-400 border border-red-500/30 hover:bg-red-500/10 transition-colors"
              >
                Unlink Account
              </button>
            </div>
          )}

          {/* Not authenticated */}
          {!isAuthenticated && (
            <div className="space-y-4">
              <div className="space-y-2">
                <p className="text-xs text-gray-400 leading-relaxed">
                  On your local machine (with a browser), run:
                </p>
                <div className="flex items-center gap-2 bg-gray-800/80 rounded-lg px-3 py-2 border border-gray-700/50">
                  <code className="text-sm text-indigo-400 font-mono flex-1 select-all">claude setup-token</code>
                  <button
                    onClick={() => navigator.clipboard.writeText('claude setup-token')}
                    className="text-gray-500 hover:text-gray-300 transition-colors shrink-0"
                    title="Copy"
                  >
                    <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
                      <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
                      <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
                    </svg>
                  </button>
                </div>
                <p className="text-xs text-gray-500 leading-relaxed">
                  This will open your browser for authentication and generate a token. Paste it below.
                </p>
              </div>

              <input
                type="password"
                value={token}
                onChange={e => setToken(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter') handleSubmitToken(); }}
                placeholder="Paste token here..."
                className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors"
                disabled={loading}
                autoFocus
              />
              <button
                onClick={handleSubmitToken}
                disabled={loading || !token.trim()}
                className="w-full px-3 py-2 rounded-lg text-sm font-medium bg-indigo-600 hover:bg-indigo-500 text-white transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              >
                {loading ? 'Linking...' : 'Link Account'}
              </button>
            </div>
          )}

          {/* Error display */}
          {error && (
            <div className="mt-3 px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/20">
              <p className="text-xs text-red-400">{error}</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
