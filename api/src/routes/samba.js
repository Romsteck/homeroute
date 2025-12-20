import { Router } from 'express';
import {
  getConfig,
  updateGlobalConfig,
  getShares,
  getShare,
  addShare,
  updateShare,
  deleteShare,
  toggleShare,
  generateSmbConf,
  applySmbConf,
  testSmbConf,
  getServiceStatus,
  restartSamba,
  reloadSamba,
  getActiveSessions,
  getOpenFiles,
  getShareConnections,
  getUsers,
  addUser,
  removeUser,
  changePassword,
  enableUser,
  disableUser,
  importFromSmbConf
} from '../services/samba.js';

const router = Router();

// ========== Configuration Endpoints ==========

// GET /api/samba/config - Configuration complète
router.get('/config', async (req, res) => {
  const result = await getConfig();
  res.json(result);
});

// PUT /api/samba/config - Modifier configuration globale
router.put('/config', async (req, res) => {
  const result = await updateGlobalConfig(req.body);
  res.json(result);
});

// ========== Service Status Endpoints ==========

// GET /api/samba/status - Statut des services smbd/nmbd
router.get('/status', async (req, res) => {
  const result = await getServiceStatus();
  res.json(result);
});

// POST /api/samba/restart - Redémarrer Samba
router.post('/restart', async (req, res) => {
  const result = await restartSamba();
  res.json(result);
});

// POST /api/samba/reload - Recharger configuration
router.post('/reload', async (req, res) => {
  const result = await reloadSamba();
  res.json(result);
});

// ========== Share Management Endpoints ==========

// GET /api/samba/shares - Liste des partages
router.get('/shares', async (req, res) => {
  const result = await getShares();
  res.json(result);
});

// GET /api/samba/shares/:id - Détails d'un partage
router.get('/shares/:id', async (req, res) => {
  const result = await getShare(req.params.id);
  res.json(result);
});

// POST /api/samba/shares - Créer un partage
router.post('/shares', async (req, res) => {
  const { name, path } = req.body;
  if (!name || !path) {
    return res.status(400).json({ success: false, error: 'Le nom et le chemin sont requis' });
  }
  const result = await addShare(req.body);
  res.json(result);
});

// PUT /api/samba/shares/:id - Modifier un partage
router.put('/shares/:id', async (req, res) => {
  const result = await updateShare(req.params.id, req.body);
  res.json(result);
});

// DELETE /api/samba/shares/:id - Supprimer un partage
router.delete('/shares/:id', async (req, res) => {
  const result = await deleteShare(req.params.id);
  res.json(result);
});

// POST /api/samba/shares/:id/toggle - Activer/désactiver un partage
router.post('/shares/:id/toggle', async (req, res) => {
  const { enabled } = req.body;
  const result = await toggleShare(req.params.id, enabled);
  res.json(result);
});

// ========== Configuration Apply Endpoints ==========

// POST /api/samba/apply - Appliquer les modifications (génère smb.conf + reload)
router.post('/apply', async (req, res) => {
  const result = await applySmbConf();
  res.json(result);
});

// POST /api/samba/testparm - Valider la configuration
router.post('/testparm', async (req, res) => {
  const result = await testSmbConf();
  res.json(result);
});

// GET /api/samba/preview - Prévisualiser le smb.conf généré
router.get('/preview', async (req, res) => {
  const result = await generateSmbConf();
  res.json(result);
});

// POST /api/samba/import - Importer les partages depuis smb.conf existant
router.post('/import', async (req, res) => {
  const result = await importFromSmbConf();
  res.json(result);
});

// ========== Monitoring Endpoints ==========

// GET /api/samba/sessions - Sessions actives
router.get('/sessions', async (req, res) => {
  const result = await getActiveSessions();
  res.json(result);
});

// GET /api/samba/files - Fichiers ouverts
router.get('/files', async (req, res) => {
  const result = await getOpenFiles();
  res.json(result);
});

// GET /api/samba/connections/:shareName - Connexions par partage
router.get('/connections/:shareName', async (req, res) => {
  const result = await getShareConnections(req.params.shareName);
  res.json(result);
});

// ========== User Management Endpoints ==========

// GET /api/samba/users - Liste des utilisateurs Samba
router.get('/users', async (req, res) => {
  const result = await getUsers();
  res.json(result);
});

// POST /api/samba/users - Ajouter un utilisateur Samba
router.post('/users', async (req, res) => {
  const { username, password } = req.body;
  if (!username || !password) {
    return res.status(400).json({ success: false, error: 'Nom d\'utilisateur et mot de passe requis' });
  }
  const result = await addUser(username, password);
  res.json(result);
});

// DELETE /api/samba/users/:username - Supprimer un utilisateur Samba
router.delete('/users/:username', async (req, res) => {
  const result = await removeUser(req.params.username);
  res.json(result);
});

// PUT /api/samba/users/:username/password - Changer le mot de passe
router.put('/users/:username/password', async (req, res) => {
  const { password } = req.body;
  if (!password) {
    return res.status(400).json({ success: false, error: 'Mot de passe requis' });
  }
  const result = await changePassword(req.params.username, password);
  res.json(result);
});

// POST /api/samba/users/:username/enable - Activer un utilisateur
router.post('/users/:username/enable', async (req, res) => {
  const result = await enableUser(req.params.username);
  res.json(result);
});

// POST /api/samba/users/:username/disable - Désactiver un utilisateur
router.post('/users/:username/disable', async (req, res) => {
  const result = await disableUser(req.params.username);
  res.json(result);
});

export default router;
