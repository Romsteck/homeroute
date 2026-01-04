import { Router } from 'express';
import {
  getCpuInfo,
  getGovernorStatus,
  setGovernor,
  getFanStatus,
  setFanSpeed,
  getFanProfiles,
  saveFanProfile,
  applyFanProfile,
  getScheduleConfig,
  saveScheduleConfig,
  applyMode,
  getCurrentMode,
  getEnergyModes,
  ENERGY_MODES
} from '../services/energy.js';

const router = Router();

// ============ CPU INFO ============

// GET /api/energy/cpu - Infos CPU (temp, freq, usage) pour polling
router.get('/cpu', async (req, res) => {
  const result = await getCpuInfo();
  res.json(result);
});

// ============ GOVERNOR ============

// GET /api/energy/status - Gouverneur actuel + disponibles
router.get('/status', async (req, res) => {
  const result = await getGovernorStatus();
  res.json(result);
});

// POST /api/energy/governor - Changer le gouverneur
router.post('/governor', async (req, res) => {
  const { governor } = req.body;

  if (!governor) {
    return res.status(400).json({ success: false, error: 'Governor is required' });
  }

  const result = await setGovernor(governor);
  res.json(result);
});

// ============ FANS ============

// GET /api/energy/fans - État des ventilateurs
router.get('/fans', async (req, res) => {
  const result = await getFanStatus();
  res.json(result);
});

// POST /api/energy/fans/:id - Modifier un ventilateur
router.post('/fans/:id', async (req, res) => {
  const { id } = req.params;
  const { pwm, mode } = req.body;

  const result = await setFanSpeed(id, pwm, mode);
  res.json(result);
});

// ============ FAN PROFILES ============

// GET /api/energy/fans/profiles - Liste des profils
router.get('/fans/profiles', async (req, res) => {
  const result = await getFanProfiles();
  res.json(result);
});

// POST /api/energy/fans/profiles - Créer/modifier un profil
router.post('/fans/profiles', async (req, res) => {
  const profile = req.body;

  if (!profile || !profile.name) {
    return res.status(400).json({ success: false, error: 'Profile name is required' });
  }

  const result = await saveFanProfile(profile);
  res.json(result);
});

// POST /api/energy/fans/profiles/:name/apply - Appliquer un profil
router.post('/fans/profiles/:name/apply', async (req, res) => {
  const { name } = req.params;
  const result = await applyFanProfile(name);
  res.json(result);
});

// ============ SCHEDULE ============

// GET /api/energy/schedule - Config de programmation
router.get('/schedule', async (req, res) => {
  const result = await getScheduleConfig();
  res.json(result);
});

// POST /api/energy/schedule - Sauvegarder la programmation
router.post('/schedule', async (req, res) => {
  const config = req.body;
  const result = await saveScheduleConfig(config);
  res.json(result);
});

// ============ ENERGY MODES ============

// GET /api/energy/modes - Liste des modes disponibles
router.get('/modes', (req, res) => {
  res.json(getEnergyModes());
});

// GET /api/energy/mode - Mode actuel
router.get('/mode', async (req, res) => {
  const result = await getCurrentMode();
  res.json(result);
});

// POST /api/energy/mode/:mode - Appliquer un mode (economy/auto/performance ou day/night)
router.post('/mode/:mode', async (req, res) => {
  const { mode } = req.params;
  const result = await applyMode(mode);
  res.json(result);
});

export default router;
