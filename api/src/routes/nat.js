import { Router } from 'express';
import { getNatRules, getFilterRules, getMasqueradeRules, getPortForwards } from '../services/firewall.js';

const router = Router();

// GET /api/nat/rules - Règles NAT complètes
router.get('/rules', async (req, res) => {
  const result = await getNatRules();
  res.json(result);
});

// GET /api/nat/filter - Règles de filtrage
router.get('/filter', async (req, res) => {
  const result = await getFilterRules();
  res.json(result);
});

// GET /api/nat/masquerade - Règles MASQUERADE
router.get('/masquerade', async (req, res) => {
  const result = await getMasqueradeRules();
  res.json(result);
});

// GET /api/nat/forwards - Port forwards (DNAT)
router.get('/forwards', async (req, res) => {
  const result = await getPortForwards();
  res.json(result);
});

export default router;
