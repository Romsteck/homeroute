import { Router } from 'express';
import {
  getConfig,
  saveConfig,
  runBackup,
  getHistory,
  testConnection,
  cancelBackup,
  isBackupRunning
} from '../services/backup.js';

const router = Router();

// GET /api/backup/config - Configuration actuelle
router.get('/config', async (req, res) => {
  const result = await getConfig();
  res.json(result);
});

// POST /api/backup/config - Sauvegarder configuration
router.post('/config', async (req, res) => {
  const { sources } = req.body;
  if (!sources || !Array.isArray(sources)) {
    return res.status(400).json({ success: false, error: 'Sources array required' });
  }
  const result = await saveConfig(sources);
  res.json(result);
});

// POST /api/backup/run - Lancer un backup
router.post('/run', async (req, res) => {
  req.setTimeout(3600000);
  const result = await runBackup();
  res.json(result);
});

// GET /api/backup/history - Historique des backups
router.get('/history', async (req, res) => {
  const result = await getHistory();
  res.json(result);
});

// POST /api/backup/test - Tester connexion SMB
router.post('/test', async (req, res) => {
  const result = await testConnection();
  res.json(result);
});

// POST /api/backup/cancel - Annuler le backup en cours
router.post('/cancel', async (req, res) => {
  const result = await cancelBackup();
  res.json(result);
});

// GET /api/backup/status - Statut du backup
router.get('/status', (req, res) => {
  res.json({ running: isBackupRunning() });
});

export default router;
