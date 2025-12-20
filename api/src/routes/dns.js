import { Router } from 'express';
import { getDnsConfig, getDhcpLeases } from '../services/dnsmasq.js';

const router = Router();

// GET /api/dns - Configuration DNS/DHCP
router.get('/', async (req, res) => {
  const result = await getDnsConfig();
  res.json(result);
});

// GET /api/dns/leases - Baux DHCP actifs
router.get('/leases', async (req, res) => {
  const result = await getDhcpLeases();
  res.json(result);
});

export default router;
