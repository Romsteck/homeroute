/**
 * Endpoints forward-auth pour le reverse proxy
 *
 * Ces endpoints permettent au proxy de verifier l'authentification
 * avant de proxifier les requetes vers les services proteges.
 */

import { Router } from 'express';
import { validateSession } from '../services/sessions.js';
import { getUser } from '../services/authUsers.js';
import { loadConfig } from '../services/reverseproxy.js';

const router = Router();

// Cache for reverse proxy config (refreshed every 30s)
let cachedConfig = null;
let configLastLoaded = 0;
const CONFIG_CACHE_TTL = 30000;

async function getProxyConfig() {
  const now = Date.now();
  if (!cachedConfig || now - configLastLoaded > CONFIG_CACHE_TTL) {
    cachedConfig = await loadConfig();
    configLastLoaded = now;
  }
  return cachedConfig;
}

/**
 * Find allowed groups for a domain from the reverse proxy config
 */
function findAllowedGroupsForDomain(config, domain) {
  if (!domain || !config) return [];

  // Check standalone hosts
  for (const host of config.hosts || []) {
    const hostDomain = host.customDomain || `${host.subdomain}.${config.baseDomain}`;
    if (hostDomain.toLowerCase() === domain.toLowerCase()) {
      return host.allowedGroups || [];
    }
  }

  // Check applications
  for (const app of config.applications || []) {
    if (!app.endpoints) continue;
    for (const [envId, envEndpoints] of Object.entries(app.endpoints)) {
      const env = (config.environments || []).find(e => e.id === envId);
      if (!env || !envEndpoints) continue;

      // Check frontend domain
      if (envEndpoints.frontend) {
        const frontendDomain = env.prefix
          ? `${app.slug}.${env.prefix}.${config.baseDomain}`
          : `${app.slug}.${config.baseDomain}`;
        if (frontendDomain.toLowerCase() === domain.toLowerCase()) {
          return app.allowedGroups || [];
        }
      }

      // Check API domains
      for (const api of envEndpoints.apis || []) {
        const apiSlug = api.slug || '';
        const hostPart = apiSlug ? `${app.slug}-${apiSlug}` : app.slug;
        const apiDomain = `${hostPart}.${env.apiPrefix}.${config.baseDomain}`;
        if (apiDomain.toLowerCase() === domain.toLowerCase()) {
          return app.allowedGroups || [];
        }
      }
    }
  }

  return [];
}

// /api/authz/forward-auth - Forward auth endpoint
// Use router.all so that POST/PUT/DELETE requests proxied through the reverse proxy's
// forward_auth (which preserves the original HTTP method) are handled correctly.
router.all('/forward-auth', async (req, res) => {
  const baseDomain = process.env.BASE_DOMAIN || 'localhost';
  const sessionId = req.cookies.auth_session;

  // Get the original URL for redirect
  const forwardedHost = req.get('X-Forwarded-Host') || req.get('host');
  const forwardedUri = req.get('X-Forwarded-Uri') || '/';
  const forwardedProto = req.get('X-Forwarded-Proto') || 'https';

  const originalUrl = `${forwardedProto}://${forwardedHost}${forwardedUri}`;
  const loginUrl = `https://auth.${baseDomain}/login?rd=${encodeURIComponent(originalUrl)}`;

  // No session cookie
  if (!sessionId) {
    // Return 401 with login redirect URL in header
    res.set('X-Auth-Redirect', loginUrl);
    return res.status(401).json({
      success: false,
      error: 'Authentication required',
      redirect: loginUrl
    });
  }

  // Validate session
  const session = validateSession(sessionId);

  if (!session) {
    res.set('X-Auth-Redirect', loginUrl);
    return res.status(401).json({
      success: false,
      error: 'Session expired',
      redirect: loginUrl
    });
  }

  // Get user info
  const user = getUser(session.userId);

  if (!user) {
    res.set('X-Auth-Redirect', loginUrl);
    return res.status(401).json({
      success: false,
      error: 'User not found',
      redirect: loginUrl
    });
  }

  // Check if user is disabled
  if (user.disabled) {
    res.set('X-Auth-Redirect', loginUrl);
    return res.status(403).json({
      success: false,
      error: 'Account disabled'
    });
  }

  // Check group-based access control
  const userGroups = user.groups || [];
  const isAdmin = userGroups.includes('admins');

  if (!isAdmin) {
    try {
      const config = await getProxyConfig();
      const allowedGroups = findAllowedGroupsForDomain(config, forwardedHost);

      if (allowedGroups.length > 0) {
        const hasAccess = allowedGroups.some(g => userGroups.includes(g));
        if (!hasAccess) {
          return res.status(403).json({
            success: false,
            error: 'Access denied: insufficient group permissions'
          });
        }
      }
    } catch (err) {
      console.error('Error checking group access:', err);
      // Fail open: if we can't load config, allow access (auth is still verified)
    }
  }

  // Set headers for downstream services
  res.set('Remote-User', user.username);
  res.set('Remote-Email', user.email || '');
  res.set('Remote-Name', user.displayname || user.username);
  res.set('Remote-Groups', userGroups.join(','));

  // Authentication successful
  res.status(200).json({
    success: true,
    user: user.username
  });
});

// /api/authz/forward-auth-optional - Auth optionnelle (ne bloque jamais)
// Retourne toujours 200, injecte les headers si authentifie
router.all('/forward-auth-optional', (req, res) => {
  const sessionId = req.cookies.auth_session;

  // Pas de session - retourner 200 sans headers user
  if (!sessionId) {
    return res.status(200).json({ authenticated: false });
  }

  // Valider la session
  const session = validateSession(sessionId);
  if (!session) {
    return res.status(200).json({ authenticated: false });
  }

  // Recuperer l'utilisateur
  const user = getUser(session.userId);
  if (!user || user.disabled) {
    return res.status(200).json({ authenticated: false });
  }

  // Utilisateur authentifie - injecter les headers
  res.set('Remote-User', user.username);
  res.set('Remote-Email', user.email || '');
  res.set('Remote-Name', user.displayname || user.username);
  res.set('Remote-Groups', (user.groups || []).join(','));

  res.status(200).json({ authenticated: true, user: user.username });
});

// GET /api/authz/verify - Simple session verification (for internal use)
router.get('/verify', (req, res) => {
  const sessionId = req.cookies.auth_session;

  if (!sessionId) {
    return res.status(401).json({ valid: false });
  }

  const session = validateSession(sessionId);

  if (!session) {
    return res.status(401).json({ valid: false });
  }

  res.json({
    valid: true,
    user_id: session.userId
  });
});

export default router;
