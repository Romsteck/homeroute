import { Router } from 'express';
import {
  getStats,
  getWhitelist,
  addToWhitelist,
  removeFromWhitelist,
  updateLists,
  searchBlocked
} from '../services/adblock.js';

const router = Router();

// GET /api/adblock/stats - Statistiques adblock
router.get('/stats', async (req, res) => {
  const result = await getStats();
  res.json(result);
});

// GET /api/adblock/whitelist - Liste blanche
router.get('/whitelist', async (req, res) => {
  const result = await getWhitelist();
  res.json(result);
});

// POST /api/adblock/whitelist - Ajouter à la whitelist
router.post('/whitelist', async (req, res) => {
  const { domain } = req.body;
  if (!domain) {
    return res.status(400).json({ success: false, error: 'Domain required' });
  }
  const result = await addToWhitelist(domain);
  res.json(result);
});

// DELETE /api/adblock/whitelist/:domain - Supprimer de la whitelist
router.delete('/whitelist/:domain', async (req, res) => {
  const { domain } = req.params;
  const result = await removeFromWhitelist(domain);
  res.json(result);
});

// POST /api/adblock/update - Déclencher mise à jour
router.post('/update', async (req, res) => {
  const result = await updateLists();
  res.json(result);
});

// GET /api/adblock/search - Rechercher un domaine bloqué
router.get('/search', async (req, res) => {
  const { q } = req.query;
  if (!q || q.length < 3) {
    return res.status(400).json({ success: false, error: 'Query must be at least 3 characters' });
  }
  const result = await searchBlocked(q);
  res.json(result);
});

export default router;
