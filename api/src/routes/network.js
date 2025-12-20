import { Router } from 'express';
import { getInterfaces, getRoutes, getIpv6Routes } from '../services/network.js';

const router = Router();

// GET /api/network/interfaces - Interfaces réseau
router.get('/interfaces', async (req, res) => {
  const result = await getInterfaces();
  res.json(result);
});

// GET /api/network/routes - Table de routage IPv4
router.get('/routes', async (req, res) => {
  const result = await getRoutes();
  res.json(result);
});

// GET /api/network/routes6 - Table de routage IPv6
router.get('/routes6', async (req, res) => {
  const result = await getIpv6Routes();
  res.json(result);
});

export default router;
