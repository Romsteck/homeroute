import { readFile, writeFile, mkdir } from 'fs/promises';
import { existsSync } from 'fs';
import { exec, spawn } from 'child_process';
import { promisify } from 'util';
import path from 'path';
import { getIO } from '../socket.js';

const execAsync = promisify(exec);

// État du backup actif
let activeBackupProcess = null;
let backupCancelled = false;

// Getters pour lire les variables après dotenv.config()
const getEnv = () => ({
  SMB_SERVER: process.env.SMB_SERVER || '',
  SMB_SHARE: process.env.SMB_SHARE || '',
  SMB_USERNAME: process.env.SMB_USERNAME || '',
  SMB_PASSWORD: process.env.SMB_PASSWORD || '',
  SMB_MOUNT_POINT: process.env.SMB_MOUNT_POINT || '/mnt/smb_backup',
  BACKUP_CONFIG_FILE: process.env.BACKUP_CONFIG_FILE || '/var/lib/server-dashboard/backup-config.json',
  BACKUP_HISTORY_FILE: process.env.BACKUP_HISTORY_FILE || '/var/lib/server-dashboard/backup-history.json'
});

async function ensureConfigDir() {
  const { BACKUP_CONFIG_FILE } = getEnv();
  const configDir = path.dirname(BACKUP_CONFIG_FILE);
  if (!existsSync(configDir)) {
    await mkdir(configDir, { recursive: true });
  }
}

export async function getConfig() {
  try {
    const env = getEnv();
    let sources = [];

    if (existsSync(env.BACKUP_CONFIG_FILE)) {
      const content = await readFile(env.BACKUP_CONFIG_FILE, 'utf-8');
      const config = JSON.parse(content);
      sources = config.sources || [];
    }

    return {
      success: true,
      config: {
        smbServer: env.SMB_SERVER,
        smbShare: env.SMB_SHARE,
        smbUsername: env.SMB_USERNAME,
        smbPasswordSet: !!env.SMB_PASSWORD,
        mountPoint: env.SMB_MOUNT_POINT,
        sources
      }
    };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function saveConfig(sources) {
  try {
    const { BACKUP_CONFIG_FILE } = getEnv();
    await ensureConfigDir();
    await writeFile(BACKUP_CONFIG_FILE, JSON.stringify({ sources }, null, 2));
    return { success: true, message: 'Configuration saved' };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

async function isMounted() {
  try {
    const { SMB_MOUNT_POINT } = getEnv();
    const { stdout } = await execAsync(`mount | grep "${SMB_MOUNT_POINT}"`);
    return !!stdout.trim();
  } catch {
    return false;
  }
}

async function mountSmb() {
  const env = getEnv();

  if (!env.SMB_SERVER || !env.SMB_SHARE) {
    throw new Error('SMB server or share not configured');
  }

  if (!existsSync(env.SMB_MOUNT_POINT)) {
    await execAsync(`sudo mkdir -p "${env.SMB_MOUNT_POINT}"`);
  }

  if (await isMounted()) {
    return { success: true, message: 'Already mounted' };
  }

  // Utiliser les options directes avec single quotes pour éviter l'interprétation du shell
  try {
    // Échapper les single quotes dans le mot de passe pour le shell
    const escapedPassword = env.SMB_PASSWORD.replace(/'/g, "'\\''");

    const mountOptions = env.SMB_USERNAME
      ? `username=${env.SMB_USERNAME},password='${escapedPassword}',vers=3.0,sec=ntlmssp,uid=$(id -u),gid=$(id -g)`
      : `guest,vers=3.0,uid=$(id -u),gid=$(id -g)`;

    const mountCmd = `sudo mount -t cifs "//${env.SMB_SERVER}/${env.SMB_SHARE}" "${env.SMB_MOUNT_POINT}" -o ${mountOptions}`;
    await execAsync(mountCmd, { timeout: 30000 });

    return { success: true, message: 'Mounted successfully' };
  } catch (error) {
    throw new Error(`Failed to mount SMB share: ${error.message}`);
  }
}

async function unmountSmb() {
  try {
    const { SMB_MOUNT_POINT } = getEnv();
    if (await isMounted()) {
      await execAsync(`sudo umount "${SMB_MOUNT_POINT}"`);
    }
    return { success: true };
  } catch (error) {
    return { success: false, message: error.message };
  }
}

export async function testConnection() {
  try {
    const { SMB_MOUNT_POINT } = getEnv();
    await mountSmb();

    const testFile = path.join(SMB_MOUNT_POINT, '.connection-test');
    await execAsync(`touch "${testFile}" && rm "${testFile}"`);

    await unmountSmb();

    return { success: true, message: 'SMB connection successful' };
  } catch (error) {
    await unmountSmb();
    return { success: false, error: error.message };
  }
}

function parseRsyncStats(output) {
  const stats = {
    filesTransferred: 0,
    totalSize: 0,
    transferredSize: 0,
    speed: ''
  };

  const filesMatch = output.match(/Number of regular files transferred: ([\d,]+)/);
  if (filesMatch) {
    stats.filesTransferred = parseInt(filesMatch[1].replace(/,/g, ''));
  }

  const sizeMatch = output.match(/Total file size: ([\d,]+)/);
  if (sizeMatch) {
    stats.totalSize = parseInt(sizeMatch[1].replace(/,/g, ''));
  }

  const transferMatch = output.match(/Total transferred file size: ([\d,]+)/);
  if (transferMatch) {
    stats.transferredSize = parseInt(transferMatch[1].replace(/,/g, ''));
  }

  const speedMatch = output.match(/([\d.]+[KMG]?B\/s)/);
  if (speedMatch) {
    stats.speed = speedMatch[1];
  }

  return stats;
}

// Parse rsync --info=progress2 output
// Format EN: "     32,768 100%    2.08MB/s    0:00:00"
// Format FR: "  1.049.919.488   0% 1001,25MB/s    0:09:03"
function parseRsyncProgress(line) {
  // Match numbers with dots or commas as thousand separators, then percentage, then speed
  const match = line.match(/^\s*([\d.,]+)\s+(\d+)%\s+([\d.,]+\s*[KMG]?B\/s)/);
  if (match) {
    // Remove thousand separators (both . and ,) but keep the number
    const bytesStr = match[1].replace(/[.,]/g, '');
    return {
      transferredBytes: parseInt(bytesStr),
      percent: parseInt(match[2]),
      speed: match[3].replace(/\s+/g, '')
    };
  }
  return null;
}

// Run rsync with spawn for streaming progress
function runRsyncWithProgress(source, destPath, sourceIndex, sourceName, sourcesCount) {
  return new Promise((resolve, reject) => {
    // Use stdbuf to force line-buffered output (rsync doesn't output progress when not on TTY)
    const args = [
      'stdbuf', '-oL',
      'rsync', '-av', '--delete', '--info=progress2', '--no-inc-recursive', '--stats',
      `${source}/`, `${destPath}/`
    ];

    const rsync = spawn('sudo', args);
    activeBackupProcess = rsync;

    let stdout = '';
    let stderr = '';

    // Function to parse and emit progress from any output
    const processOutput = (chunk) => {
      // Split by carriage return or newline
      const lines = chunk.split(/[\r\n]+/);
      for (const line of lines) {
        if (line.includes('%')) {
          console.log('[rsync progress line]', JSON.stringify(line));
        }
        const progress = parseRsyncProgress(line);
        if (progress) {
          console.log('[rsync progress parsed]', progress);
          getIO().emit('backup:progress', {
            sourceIndex,
            sourceName,
            sourcesCount,
            percent: progress.percent,
            transferredBytes: progress.transferredBytes,
            speed: progress.speed
          });
        }
      }
    };

    rsync.stdout.on('data', (data) => {
      const chunk = data.toString();
      stdout += chunk;
      processOutput(chunk);
    });

    rsync.stderr.on('data', (data) => {
      const chunk = data.toString();
      stderr += chunk;
      // rsync sometimes outputs progress to stderr
      processOutput(chunk);
    });

    rsync.on('close', (code) => {
      activeBackupProcess = null;
      if (backupCancelled) {
        reject(new Error('Backup cancelled by user'));
      } else if (code === 0) {
        resolve({ stdout, stderr });
      } else {
        reject(new Error(`rsync exited with code ${code}: ${stderr}`));
      }
    });

    rsync.on('error', (err) => {
      activeBackupProcess = null;
      reject(err);
    });
  });
}

export async function runBackup() {
  const startTime = Date.now();
  const timestamp = new Date().toISOString();
  const { SMB_MOUNT_POINT } = getEnv();

  // Reset cancellation state
  backupCancelled = false;

  try {
    const { config } = await getConfig();
    const sources = config.sources;

    if (!sources || sources.length === 0) {
      return { success: false, error: 'No backup sources configured' };
    }

    const validSources = sources.filter(src => existsSync(src));
    if (validSources.length === 0) {
      return { success: false, error: 'No valid backup sources found' };
    }

    await mountSmb();

    // Emit backup started
    getIO().emit('backup:started', {
      timestamp,
      sourcesCount: validSources.length,
      sources: validSources.map(s => path.basename(s))
    });

    const results = [];
    let totalFiles = 0;
    let totalTransferred = 0;

    for (let i = 0; i < validSources.length; i++) {
      if (backupCancelled) break;

      const source = validSources[i];
      const sourceName = path.basename(source);
      const destPath = path.join(SMB_MOUNT_POINT, sourceName);

      // Emit source start
      getIO().emit('backup:source-start', {
        sourceIndex: i,
        sourceName,
        sourcePath: source,
        sourcesCount: validSources.length
      });

      try {
        const { stdout } = await runRsyncWithProgress(source, destPath, i, sourceName, validSources.length);
        const stats = parseRsyncStats(stdout);

        results.push({
          source,
          success: true,
          filesTransferred: stats.filesTransferred,
          transferredSize: stats.transferredSize
        });

        totalFiles += stats.filesTransferred;
        totalTransferred += stats.transferredSize;

        // Emit source complete
        getIO().emit('backup:source-complete', {
          sourceIndex: i,
          sourceName,
          filesTransferred: stats.filesTransferred,
          transferredSize: stats.transferredSize
        });
      } catch (error) {
        if (backupCancelled) {
          results.push({
            source,
            success: false,
            error: 'Cancelled'
          });
        } else {
          results.push({
            source,
            success: false,
            error: error.message
          });
        }
      }
    }

    await unmountSmb();

    const duration = Date.now() - startTime;
    const allSuccess = results.every(r => r.success);
    const status = backupCancelled ? 'cancelled' : (allSuccess ? 'success' : 'partial');

    await addToHistory({
      timestamp,
      duration,
      status,
      sourcesCount: validSources.length,
      filesTransferred: totalFiles,
      transferredSize: totalTransferred,
      results
    });

    // Emit backup complete
    getIO().emit('backup:complete', {
      success: !backupCancelled && allSuccess,
      cancelled: backupCancelled,
      duration,
      totalFiles,
      totalSize: totalTransferred,
      results
    });

    if (backupCancelled) {
      return { success: false, error: 'Backup cancelled by user' };
    }

    return {
      success: true,
      message: allSuccess ? 'Backup completed successfully' : 'Backup completed with some errors',
      details: {
        duration,
        sourcesBackedUp: validSources.length,
        filesTransferred: totalFiles,
        transferredSize: totalTransferred,
        results
      }
    };
  } catch (error) {
    await unmountSmb();

    await addToHistory({
      timestamp,
      duration: Date.now() - startTime,
      status: 'failed',
      error: error.message
    });

    // Emit error
    getIO().emit('backup:error', { error: error.message });

    return { success: false, error: error.message };
  }
}

export async function cancelBackup() {
  if (!activeBackupProcess) {
    return { success: false, error: 'No backup in progress' };
  }

  backupCancelled = true;

  try {
    // Kill the rsync process (need to kill sudo and its child)
    await execAsync(`sudo pkill -P ${activeBackupProcess.pid}`);
    activeBackupProcess.kill('SIGTERM');
  } catch {
    // Process may have already exited
  }

  getIO().emit('backup:cancelled', { reason: 'user' });

  return { success: true, message: 'Backup cancellation requested' };
}

export function isBackupRunning() {
  return activeBackupProcess !== null;
}

export async function getHistory() {
  try {
    const { BACKUP_HISTORY_FILE } = getEnv();

    if (!existsSync(BACKUP_HISTORY_FILE)) {
      return { success: true, history: [] };
    }

    const content = await readFile(BACKUP_HISTORY_FILE, 'utf-8');
    const history = JSON.parse(content);

    return { success: true, history };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

async function addToHistory(entry) {
  try {
    const { BACKUP_HISTORY_FILE } = getEnv();
    await ensureConfigDir();

    let history = [];
    if (existsSync(BACKUP_HISTORY_FILE)) {
      const content = await readFile(BACKUP_HISTORY_FILE, 'utf-8');
      history = JSON.parse(content);
    }

    history.unshift(entry);
    history = history.slice(0, 50);

    await writeFile(BACKUP_HISTORY_FILE, JSON.stringify(history, null, 2));
  } catch (error) {
    console.error('Failed to save backup history:', error);
  }
}
