import { Router } from 'express';
import { readFile, writeFile } from 'fs/promises';
import { existsSync } from 'fs';
import { exec } from 'child_process';
import { promisify } from 'util';

const execAsync = promisify(exec);
const router = Router();

const CONFIG_PATH = process.env.DNS_DHCP_CONFIG_PATH || '/var/lib/server-dashboard/dns-dhcp-config.json';

// GET /api/dns-dhcp/status - Service status
router.get('/status', async (req, res) => {
  try {
    const { stdout } = await execAsync('systemctl is-active rust-dns-dhcp 2>/dev/null || echo inactive');
    const active = stdout.trim() === 'active';
    res.json({ success: true, active, status: stdout.trim() });
  } catch (error) {
    res.json({ success: false, error: error.message });
  }
});

// POST /api/dns-dhcp/reload - Send SIGHUP for hot-reload
router.post('/reload', async (req, res) => {
  try {
    await execAsync('systemctl reload rust-dns-dhcp');
    res.json({ success: true, message: 'Reload signal sent' });
  } catch (error) {
    res.json({ success: false, error: error.message });
  }
});

// GET /api/dns-dhcp/config - Read config
router.get('/config', async (req, res) => {
  try {
    if (!existsSync(CONFIG_PATH)) {
      return res.json({ success: false, error: 'Config file not found' });
    }
    const content = await readFile(CONFIG_PATH, 'utf-8');
    res.json({ success: true, config: JSON.parse(content) });
  } catch (error) {
    res.json({ success: false, error: error.message });
  }
});

// PUT /api/dns-dhcp/config - Update config + reload
router.put('/config', async (req, res) => {
  try {
    const config = req.body;
    if (!config || typeof config !== 'object') {
      return res.status(400).json({ success: false, error: 'Invalid config' });
    }
    await writeFile(CONFIG_PATH, JSON.stringify(config, null, 2));
    // Trigger hot-reload
    try {
      await execAsync('systemctl reload rust-dns-dhcp');
    } catch {
      // Service might not be running
    }
    res.json({ success: true, message: 'Config updated and reload triggered' });
  } catch (error) {
    res.json({ success: false, error: error.message });
  }
});

export default router;
