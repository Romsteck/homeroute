import { readFile } from 'fs/promises';
import { existsSync } from 'fs';
import { exec } from 'child_process';
import { promisify } from 'util';

const execAsync = promisify(exec);

const DDNS_CONFIG = process.env.DDNS_CONFIG || '/etc/cloudflare-ddns.conf';
const DDNS_LOG = process.env.DDNS_LOG || '/var/log/cloudflare-ddns-v6.log';
const DDNS_SCRIPT = process.env.DDNS_SCRIPT || '/usr/local/bin/cloudflare-ddns-v6.sh';

export async function getStatus() {
  try {
    let config = {};
    let currentIpv6 = null;
    let logs = [];

    // Read config (mask token)
    if (existsSync(DDNS_CONFIG)) {
      const content = await readFile(DDNS_CONFIG, 'utf-8');
      const lines = content.split('\n');

      for (const line of lines) {
        if (line.startsWith('CF_API_TOKEN=')) {
          config.apiToken = '***masked***';
        } else if (line.startsWith('CF_ZONE_ID=')) {
          config.zoneId = line.split('=')[1].replace(/"/g, '');
        } else if (line.startsWith('CF_RECORD_NAME=')) {
          config.recordName = line.split('=')[1].replace(/"/g, '');
        }
      }
    }

    // Get current IPv6
    try {
      const { stdout } = await execAsync("ip -6 addr show enp5s0 scope global | grep -oP '2a0d:[0-9a-f:]+(?=/)' | head -1");
      currentIpv6 = stdout.trim() || null;
    } catch {
      // No IPv6 address
    }

    // Get recent logs
    if (existsSync(DDNS_LOG)) {
      const { stdout } = await execAsync(`tail -30 ${DDNS_LOG}`);
      logs = stdout.split('\n').filter(l => l.trim()).reverse();
    }

    // Parse last update from logs
    let lastUpdate = null;
    let lastIp = null;
    for (const log of logs) {
      const match = log.match(/^(.+): (MAJ|CREE) - .+ -> (.+)$/);
      if (match) {
        lastUpdate = match[1];
        lastIp = match[3];
        break;
      }
    }

    return {
      success: true,
      status: {
        config,
        currentIpv6,
        lastUpdate,
        lastIp,
        logs
      }
    };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function forceUpdate() {
  try {
    if (!existsSync(DDNS_SCRIPT)) {
      return { success: false, error: 'DDNS script not found' };
    }

    const { stdout, stderr } = await execAsync(`sudo ${DDNS_SCRIPT}`, { timeout: 30000 });

    // Read updated status
    const status = await getStatus();

    return {
      success: true,
      message: 'Update triggered',
      output: stdout + stderr,
      status: status.success ? status.status : null
    };
  } catch (error) {
    return { success: false, error: error.message };
  }
}
