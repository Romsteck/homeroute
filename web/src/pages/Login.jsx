import { useState, useMemo } from 'react';
import { Lock, User, AlertCircle, Globe } from 'lucide-react';
import { login } from '../api/client';

function Login() {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  // Determine redirect behavior
  const { isProxyDomain, targetDomain, redirectUrl } = useMemo(() => {
    const hostname = window.location.hostname;
    const params = new URLSearchParams(window.location.search);
    const redirectParam = params.get('redirect');

    // Check if we're on the main proxy domain (proxy.xxx.xxx)
    const isProxy = hostname.startsWith('proxy.');

    if (redirectParam) {
      // Legacy: redirect param provided
      return { isProxyDomain: true, targetDomain: redirectParam, redirectUrl: redirectParam };
    } else if (!isProxy) {
      // We're on a protected subdomain - reload after login
      return { isProxyDomain: false, targetDomain: hostname, redirectUrl: window.location.href };
    } else {
      // On proxy domain, go to dashboard
      return { isProxyDomain: true, targetDomain: null, redirectUrl: '/' };
    }
  }, []);

  async function handleSubmit(e) {
    e.preventDefault();
    setError('');
    setLoading(true);

    try {
      const res = await login(username, password);
      if (res.data.success) {
        // Reload current page (cookie is now set, will be proxied to target)
        window.location.href = redirectUrl;
      } else {
        setError(res.data.error || 'Connexion echouee');
      }
    } catch (err) {
      setError(err.response?.data?.error || 'Erreur de connexion au serveur');
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="min-h-screen bg-gray-900 flex items-center justify-center p-4">
      <div className="bg-gray-800 rounded-lg p-8 w-full max-w-md border border-gray-700 shadow-xl">
        <div className="text-center mb-6">
          <div className="w-16 h-16 bg-blue-600/20 rounded-full flex items-center justify-center mx-auto mb-4">
            <Lock className="w-8 h-8 text-blue-400" />
          </div>
          <h1 className="text-2xl font-bold text-white">Connexion requise</h1>
          <p className="text-gray-400 text-sm mt-1">
            Authentifiez-vous pour acceder a ce service
          </p>
        </div>

        {error && (
          <div className="bg-red-900/50 border border-red-600 rounded-lg p-3 mb-4 flex items-center gap-2 text-red-400">
            <AlertCircle className="w-5 h-5 flex-shrink-0" />
            <span className="text-sm">{error}</span>
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-sm text-gray-400 mb-1">
              Nom d&apos;utilisateur
            </label>
            <div className="relative">
              <User className="w-5 h-5 text-gray-500 absolute left-3 top-2.5" />
              <input
                type="text"
                value={username}
                onChange={e => setUsername(e.target.value)}
                className="w-full pl-10 pr-3 py-2 bg-gray-900 border border-gray-600 rounded-lg text-white focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
                placeholder="admin"
                autoComplete="username"
                required
              />
            </div>
          </div>

          <div>
            <label className="block text-sm text-gray-400 mb-1">
              Mot de passe
            </label>
            <div className="relative">
              <Lock className="w-5 h-5 text-gray-500 absolute left-3 top-2.5" />
              <input
                type="password"
                value={password}
                onChange={e => setPassword(e.target.value)}
                className="w-full pl-10 pr-3 py-2 bg-gray-900 border border-gray-600 rounded-lg text-white focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
                placeholder="••••••••"
                autoComplete="current-password"
                required
              />
            </div>
          </div>

          <button
            type="submit"
            disabled={loading}
            className="w-full py-2.5 bg-blue-600 hover:bg-blue-700 disabled:bg-blue-800 disabled:cursor-not-allowed rounded-lg font-medium text-white transition-colors"
          >
            {loading ? (
              <span className="flex items-center justify-center gap-2">
                <svg className="animate-spin h-5 w-5" viewBox="0 0 24 24">
                  <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
                  <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                </svg>
                Connexion...
              </span>
            ) : (
              'Se connecter'
            )}
          </button>
        </form>

        {targetDomain && (
          <div className="mt-4 pt-4 border-t border-gray-700">
            <div className="flex items-center justify-center gap-2 text-gray-400">
              <Globe className="w-4 h-4" />
              <span className="text-sm">{targetDomain}</span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default Login;
