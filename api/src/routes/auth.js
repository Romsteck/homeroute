import { Router } from 'express';

const router = Router();

/**
 * GET /api/auth/me - Info utilisateur courant via headers Authelia
 *
 * Retourne l'utilisateur authentifié via les headers transmis par Caddy forward_auth
 * Headers lus: Remote-User, Remote-Groups, Remote-Email, Remote-Name
 */
router.get('/me', (req, res) => {
  if (req.autheliaUser) {
    res.json({
      success: true,
      user: {
        username: req.autheliaUser.username,
        displayName: req.autheliaUser.displayName,
        email: req.autheliaUser.email,
        groups: req.autheliaUser.groups,
        isAdmin: req.autheliaUser.isAdmin
      },
      authMethod: 'authelia'
    });
  } else {
    res.json({
      success: false,
      user: null,
      authUrl: 'https://auth.mynetwk.biz'
    });
  }
});

/**
 * POST /api/auth/logout - Déconnexion via Authelia
 *
 * Retourne l'URL de logout Authelia pour que le frontend redirige l'utilisateur
 */
router.post('/logout', (req, res) => {
  // Construire l'URL de redirection après logout
  const redirectUrl = req.get('X-Original-URL') || 'https://proxy.mynetwk.biz';

  res.json({
    success: true,
    logoutUrl: `https://auth.mynetwk.biz/logout?rd=${encodeURIComponent(redirectUrl)}`
  });
});

export default router;
