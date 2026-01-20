/**
 * Middleware d'authentification via auth-service
 *
 * Vérifie le cookie auth_session auprès de auth-service (port 9100)
 * et peuple req.autheliaUser avec les infos utilisateur
 */

import http from 'http';

const AUTH_SERVICE_URL = process.env.AUTH_SERVICE_URL || 'http://localhost:9100';

/**
 * Vérifie la session auprès de auth-service
 * @param {string} sessionCookie - Valeur du cookie auth_session
 * @returns {Promise<object|null>} - User info ou null si invalide
 */
async function verifySessionWithAuthService(sessionCookie) {
  return new Promise((resolve) => {
    const url = new URL('/api/authz/forward-auth', AUTH_SERVICE_URL);

    const options = {
      hostname: url.hostname,
      port: url.port || 9100,
      path: url.pathname,
      method: 'GET',
      headers: {
        'Cookie': `auth_session=${sessionCookie}`,
        'X-Forwarded-Host': 'proxy.mynetwk.biz',
        'X-Forwarded-Proto': 'https'
      },
      timeout: 5000
    };

    const req = http.request(options, (res) => {
      let body = '';
      res.on('data', chunk => body += chunk);
      res.on('end', () => {
        if (res.statusCode === 200) {
          // Récupérer les headers Remote-* de la réponse
          const user = {
            username: res.headers['remote-user'],
            email: res.headers['remote-email'] || '',
            displayName: res.headers['remote-name'] || res.headers['remote-user'],
            groups: res.headers['remote-groups'] ? res.headers['remote-groups'].split(',').map(g => g.trim()) : []
          };

          if (user.username) {
            user.isAdmin = user.groups.includes('admins');
            user.isPowerUser = user.groups.includes('power_users');
            user.hasGroup = (group) => user.groups.includes(group);
            resolve(user);
          } else {
            resolve(null);
          }
        } else {
          resolve(null);
        }
      });
    });

    req.on('error', (err) => {
      console.error('Auth service verification error:', err.message);
      resolve(null);
    });

    req.on('timeout', () => {
      req.destroy();
      resolve(null);
    });

    req.end();
  });
}

/**
 * Middleware global qui vérifie le cookie auth_session via auth-service
 * Ne bloque pas - peuple simplement req.autheliaUser si authentifié
 */
export async function autheliaAuth(req, res, next) {
  // Récupérer le cookie auth_session
  const sessionCookie = req.cookies?.auth_session;

  if (sessionCookie) {
    try {
      const user = await verifySessionWithAuthService(sessionCookie);
      if (user) {
        req.autheliaUser = user;
      }
    } catch (err) {
      console.error('Auth verification error:', err);
    }
  }

  next();
}

/**
 * Middleware qui exige une authentification
 * Retourne 401 avec URL de redirection si non authentifié
 */
export function requireAuth(req, res, next) {
  if (!req.autheliaUser) {
    const originalUrl = req.get('X-Original-URL') || `https://proxy.mynetwk.biz${req.originalUrl}`;
    return res.status(401).json({
      success: false,
      error: 'Authentication required',
      authUrl: 'https://auth.mynetwk.biz',
      redirect: `https://auth.mynetwk.biz/login?rd=${encodeURIComponent(originalUrl)}`
    });
  }
  next();
}

/**
 * Middleware factory qui exige un ou plusieurs groupes spécifiques
 * @param {...string} groups - Groupes autorisés (au moins un doit correspondre)
 */
export function requireGroup(...groups) {
  return (req, res, next) => {
    if (!req.autheliaUser) {
      return res.status(401).json({
        success: false,
        error: 'Authentication required',
        authUrl: 'https://auth.mynetwk.biz'
      });
    }

    const userGroups = req.autheliaUser.groups;
    const hasRequiredGroup = groups.some(g => userGroups.includes(g));

    if (!hasRequiredGroup) {
      return res.status(403).json({
        success: false,
        error: 'Insufficient permissions',
        requiredGroups: groups,
        userGroups: userGroups
      });
    }
    next();
  };
}

/**
 * Raccourci pour exiger le groupe admins
 */
export const requireAdmin = requireGroup('admins');

/**
 * Raccourci pour exiger admins ou power_users
 */
export const requirePowerUser = requireGroup('admins', 'power_users');

export default {
  autheliaAuth,
  requireAuth,
  requireGroup,
  requireAdmin,
  requirePowerUser
};
