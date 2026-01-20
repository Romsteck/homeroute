import { createContext, useContext, useState, useEffect, useCallback } from 'react';
import { getMe, logout as apiLogout } from '../api/client';

const AuthContext = createContext(null);

const AUTH_URL = 'https://auth.mynetwk.biz';

export function AuthProvider({ children }) {
  const [user, setUser] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);

  const checkAuth = useCallback(async () => {
    try {
      const res = await getMe();

      if (res.data.success && res.data.user) {
        setUser(res.data.user);
        setError(null);
      } else {
        // Pas d'utilisateur connecté - rediriger vers auth-service
        const currentUrl = window.location.href;
        window.location.href = `${AUTH_URL}/login?rd=${encodeURIComponent(currentUrl)}`;
        return; // Ne pas continuer
      }
    } catch (err) {
      console.error('Auth check failed:', err);
      // En cas d'erreur, rediriger vers auth-service
      const currentUrl = window.location.href;
      window.location.href = `${AUTH_URL}/login?rd=${encodeURIComponent(currentUrl)}`;
      return;
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    checkAuth();
  }, [checkAuth]);

  const logout = useCallback(async () => {
    try {
      const res = await apiLogout();
      if (res.data.logoutUrl) {
        window.location.href = res.data.logoutUrl;
      } else {
        // Fallback si pas d'URL retournée
        window.location.href = `${AUTH_URL}/logout`;
      }
    } catch (err) {
      console.error('Logout failed:', err);
      // Fallback direct vers auth-service logout
      window.location.href = `${AUTH_URL}/logout`;
    }
  }, []);

  // Pendant le chargement, afficher un spinner simple
  if (loading) {
    return (
      <div className="min-h-screen bg-gray-900 flex items-center justify-center">
        <div className="text-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500 mx-auto"></div>
          <p className="mt-4 text-gray-400">Chargement...</p>
        </div>
      </div>
    );
  }

  // Si pas d'utilisateur après le chargement, ne rien afficher (redirection en cours)
  if (!user) {
    return (
      <div className="min-h-screen bg-gray-900 flex items-center justify-center">
        <div className="text-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500 mx-auto"></div>
          <p className="mt-4 text-gray-400">Redirection vers l'authentification...</p>
        </div>
      </div>
    );
  }

  return (
    <AuthContext.Provider value={{ user, loading, error, logout, checkAuth }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const context = useContext(AuthContext);
  if (!context) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
}

export default AuthContext;
