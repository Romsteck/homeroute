import { useState, useEffect, useCallback } from 'react';

export default function useClaudeAuth(subscribe, sendRaw, connected) {
  const [authStatus, setAuthStatus] = useState(null);
  const [showAuthDialog, setShowAuthDialog] = useState(false);

  useEffect(() => {
    if (!subscribe) return;

    const unsubs = [
      subscribe('auth_status', (data) => {
        setAuthStatus(data);
      }),
      subscribe('oauth_url', (data) => {
        // Dispatched to AuthDialog via authEvent state
        setAuthEvent(data);
      }),
      subscribe('oauth_success', (data) => {
        setAuthStatus({ authenticated: true, ...data });
        setShowAuthDialog(false);
      }),
      subscribe('oauth_error', (data) => {
        setAuthEvent(data);
      }),
    ];

    return () => unsubs.forEach(fn => fn());
  }, [subscribe]);

  // Request auth status when connected
  useEffect(() => {
    if (connected && sendRaw) {
      sendRaw({ type: 'get_auth_status' });
    }
  }, [connected, sendRaw]);

  // Extra event state for oauth_url / oauth_error forwarding to dialog
  const [authEvent, setAuthEvent] = useState(null);

  const openAuthDialog = useCallback(() => {
    setAuthEvent(null);
    setShowAuthDialog(true);
  }, []);

  const closeAuthDialog = useCallback(() => {
    setAuthEvent(null);
    setShowAuthDialog(false);
  }, []);

  const unlinkAuth = useCallback(() => {
    if (sendRaw) {
      sendRaw({ type: 'unlink_auth' });
    }
  }, [sendRaw]);

  return {
    authStatus,
    authEvent,
    showAuthDialog,
    openAuthDialog,
    closeAuthDialog,
    unlinkAuth,
    setAuthEvent,
  };
}
