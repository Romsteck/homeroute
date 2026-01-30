import { Router } from 'express';
import { getStatus, forceUpdate, updateToken } from '../services/cloudflare.js';

const router = Router();

// GET /api/ddns/status - Status DDNS Cloudflare
router.get('/status', async (req, res) => {
  const result = await getStatus();
  res.json(result);
});

// POST /api/ddns/update - Forcer mise à jour
router.post('/update', async (req, res) => {
  const result = await forceUpdate();
  res.json(result);
});

// PUT /api/ddns/token - Mettre à jour le token API Cloudflare
router.put('/token', async (req, res) => {
  const { token } = req.body;
  if (!token || typeof token !== 'string' || !token.trim()) {
    return res.status(400).json({ success: false, error: 'Token requis' });
  }
  const result = await updateToken(token.trim());
  if (result.success) {
    const status = await getStatus();
    res.json({ ...result, status: status.success ? status.status : null });
  } else {
    res.status(500).json(result);
  }
});

export default router;
