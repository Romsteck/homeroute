import { Router } from 'express';
import { getStatus, forceUpdate } from '../services/cloudflare.js';

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

export default router;
