import { Router } from 'express';
import bcrypt from 'bcrypt';
import { loadConfig } from '../services/reverseproxy.js';

const router = Router();

// POST /api/auth/login - Connexion
router.post('/login', async (req, res) => {
  try {
    const { username, password } = req.body;

    if (!username || !password) {
      return res.status(400).json({ success: false, error: 'Username and password required' });
    }

    const config = await loadConfig();
    const account = config.authAccounts?.find(a => a.username === username.toLowerCase());

    if (!account) {
      return res.status(401).json({ success: false, error: 'Invalid credentials' });
    }

    const isValid = await bcrypt.compare(password, account.passwordHash);
    if (!isValid) {
      return res.status(401).json({ success: false, error: 'Invalid credentials' });
    }

    req.session.user = { id: account.id, username: account.username };

    // Sauvegarder explicitement la session avant de repondre
    req.session.save((err) => {
      if (err) {
        console.error('Session save error:', err);
        return res.status(500).json({ success: false, error: 'Session error' });
      }
      res.json({ success: true, user: req.session.user });
    });
  } catch (error) {
    console.error('Login error:', error);
    res.status(500).json({ success: false, error: 'Server error' });
  }
});

// POST /api/auth/logout - Deconnexion
router.post('/logout', (req, res) => {
  req.session.destroy((err) => {
    if (err) {
      return res.status(500).json({ success: false, error: 'Logout failed' });
    }
    res.clearCookie('dashboard.sid');
    res.json({ success: true });
  });
});

// GET /api/auth/check - Verification session (pour Caddy forward_auth)
router.get('/check', (req, res) => {
  if (req.session?.user) {
    res.status(200).json({ authenticated: true, user: req.session.user });
  } else {
    res.status(401).json({ authenticated: false });
  }
});

// GET /api/auth/me - Info utilisateur courant
router.get('/me', (req, res) => {
  if (req.session?.user) {
    res.json({ success: true, user: req.session.user });
  } else {
    res.json({ success: false, user: null });
  }
});

export default router;
