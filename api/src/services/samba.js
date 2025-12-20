import { readFile, writeFile, mkdir, copyFile } from 'fs/promises';
import { existsSync } from 'fs';
import path from 'path';
import crypto from 'crypto';
import { exec, spawn } from 'child_process';
import { promisify } from 'util';

const execAsync = promisify(exec);

// Environment configuration
const getEnv = () => ({
  CONFIG_FILE: process.env.SAMBA_CONFIG_FILE || '/var/lib/server-dashboard/samba-config.json',
  SMB_CONF_PATH: process.env.SMB_CONF_PATH || '/etc/samba/smb.conf',
  SMB_CONF_BACKUP: process.env.SMB_CONF_BACKUP || '/etc/samba/smb.conf.dashboard.bak'
});

// Default configuration
const getDefaultConfig = () => ({
  globalConfig: {
    workgroup: 'WORKGROUP',
    serverString: 'Samba Server %v',
    security: 'user',
    mapToGuest: 'never',
    logFile: '/var/log/samba/log.%m',
    maxLogSize: 1000
  },
  shares: []
});

async function ensureConfigDir() {
  const { CONFIG_FILE } = getEnv();
  const configDir = path.dirname(CONFIG_FILE);
  if (!existsSync(configDir)) {
    await mkdir(configDir, { recursive: true });
  }
}

// ========== Configuration Management ==========

async function loadConfig() {
  const { CONFIG_FILE } = getEnv();

  if (!existsSync(CONFIG_FILE)) {
    return getDefaultConfig();
  }

  try {
    const content = await readFile(CONFIG_FILE, 'utf-8');
    const saved = JSON.parse(content);
    return { ...getDefaultConfig(), ...saved };
  } catch {
    return getDefaultConfig();
  }
}

async function saveConfigFile(config) {
  const { CONFIG_FILE } = getEnv();
  await ensureConfigDir();
  await writeFile(CONFIG_FILE, JSON.stringify(config, null, 2));
}

export async function getConfig() {
  try {
    const config = await loadConfig();
    return {
      success: true,
      config: {
        globalConfig: config.globalConfig,
        shares: config.shares,
        sharesCount: config.shares.length
      }
    };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

// ========== Import from existing smb.conf ==========

export async function importFromSmbConf() {
  try {
    const { SMB_CONF_PATH } = getEnv();

    if (!existsSync(SMB_CONF_PATH)) {
      return { success: false, error: 'smb.conf non trouvé' };
    }

    const content = await readFile(SMB_CONF_PATH, 'utf-8');
    const parsedShares = parseSmbConf(content);

    if (parsedShares.length === 0) {
      return { success: true, message: 'Aucun partage trouvé dans smb.conf', imported: 0 };
    }

    const config = await loadConfig();
    let imported = 0;

    for (const parsed of parsedShares) {
      // Skip if already exists
      const exists = config.shares.some(s => s.name.toLowerCase() === parsed.name.toLowerCase());
      if (exists) continue;

      const newShare = {
        id: crypto.randomUUID(),
        name: parsed.name,
        path: parsed.path || '',
        comment: parsed.comment || '',
        browseable: parsed.browseable !== false,
        writable: parsed.writable === true,
        guestOk: parsed.guestOk === true,
        validUsers: parsed.validUsers || [],
        writeList: parsed.writeList || [],
        createMask: parsed.createMask || '0644',
        directoryMask: parsed.directoryMask || '0755',
        forceUser: parsed.forceUser || null,
        forceGroup: parsed.forceGroup || null,
        enabled: true,
        createdAt: new Date().toISOString(),
        importedFrom: 'smb.conf'
      };

      config.shares.push(newShare);
      imported++;
    }

    await saveConfigFile(config);

    return {
      success: true,
      message: `${imported} partage(s) importé(s)`,
      imported,
      shares: config.shares
    };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

function parseSmbConf(content) {
  const shares = [];
  const lines = content.split('\n');
  let currentSection = null;
  let currentShare = null;

  // System sections to skip
  const systemSections = ['global', 'printers', 'print$', 'homes', 'netlogon', 'profiles'];

  for (let line of lines) {
    // Remove comments
    const commentIndex = line.indexOf(';');
    if (commentIndex === 0) continue;
    const hashIndex = line.indexOf('#');
    if (hashIndex === 0) continue;

    line = line.trim();
    if (!line) continue;

    // Section header
    const sectionMatch = line.match(/^\[([^\]]+)\]$/);
    if (sectionMatch) {
      // Save previous share if valid
      if (currentShare && currentShare.path) {
        shares.push(currentShare);
      }

      currentSection = sectionMatch[1].trim();

      // Skip system sections
      if (systemSections.includes(currentSection.toLowerCase())) {
        currentShare = null;
        continue;
      }

      currentShare = { name: currentSection };
      continue;
    }

    // Parse key = value
    if (currentShare) {
      const eqIndex = line.indexOf('=');
      if (eqIndex > 0) {
        const key = line.substring(0, eqIndex).trim().toLowerCase();
        const value = line.substring(eqIndex + 1).trim();

        switch (key) {
          case 'path':
            currentShare.path = value;
            break;
          case 'comment':
            currentShare.comment = value;
            break;
          case 'browseable':
          case 'browsable':
            currentShare.browseable = value.toLowerCase() === 'yes';
            break;
          case 'writable':
          case 'writeable':
          case 'write ok':
            currentShare.writable = value.toLowerCase() === 'yes';
            break;
          case 'read only':
            currentShare.writable = value.toLowerCase() !== 'yes';
            break;
          case 'guest ok':
          case 'public':
            currentShare.guestOk = value.toLowerCase() === 'yes';
            break;
          case 'valid users':
            currentShare.validUsers = value.split(/[,\s]+/).filter(Boolean);
            break;
          case 'write list':
            currentShare.writeList = value.split(/[,\s]+/).filter(Boolean);
            break;
          case 'create mask':
          case 'create mode':
            currentShare.createMask = value;
            break;
          case 'directory mask':
          case 'directory mode':
            currentShare.directoryMask = value;
            break;
          case 'force user':
            currentShare.forceUser = value;
            break;
          case 'force group':
            currentShare.forceGroup = value;
            break;
        }
      }
    }
  }

  // Don't forget the last share
  if (currentShare && currentShare.path) {
    shares.push(currentShare);
  }

  return shares;
}

export async function updateGlobalConfig(globalConfig) {
  try {
    const config = await loadConfig();
    config.globalConfig = { ...config.globalConfig, ...globalConfig };
    await saveConfigFile(config);
    return { success: true, message: 'Configuration globale mise à jour', globalConfig: config.globalConfig };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

// ========== Share Management ==========

export async function getShares() {
  try {
    const config = await loadConfig();
    return { success: true, shares: config.shares || [] };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function getShare(id) {
  try {
    const config = await loadConfig();
    const share = config.shares.find(s => s.id === id);
    if (!share) {
      return { success: false, error: 'Partage non trouvé' };
    }
    return { success: true, share };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function addShare(shareConfig) {
  try {
    const { name, path: sharePath, comment, browseable, writable, guestOk, validUsers, writeList, createMask, directoryMask } = shareConfig;

    // Validation
    if (!name || !sharePath) {
      return { success: false, error: 'Le nom et le chemin sont requis' };
    }

    // Validate share name (alphanumeric, underscore, hyphen only)
    const nameRegex = /^[a-zA-Z0-9_-]+$/;
    if (!nameRegex.test(name)) {
      return { success: false, error: 'Le nom du partage ne peut contenir que des lettres, chiffres, tirets et underscores' };
    }

    // Validate path is absolute
    if (!path.isAbsolute(sharePath)) {
      return { success: false, error: 'Le chemin doit être absolu' };
    }

    const config = await loadConfig();

    // Check for duplicate name
    const exists = config.shares.some(s => s.name.toLowerCase() === name.toLowerCase());
    if (exists) {
      return { success: false, error: 'Un partage avec ce nom existe déjà' };
    }

    const newShare = {
      id: crypto.randomUUID(),
      name: name.toLowerCase(),
      path: sharePath,
      comment: comment || '',
      browseable: browseable !== false,
      writable: writable === true,
      guestOk: guestOk === true,
      validUsers: Array.isArray(validUsers) ? validUsers : [],
      writeList: Array.isArray(writeList) ? writeList : [],
      createMask: createMask || '0644',
      directoryMask: directoryMask || '0755',
      enabled: true,
      createdAt: new Date().toISOString()
    };

    config.shares.push(newShare);
    await saveConfigFile(config);

    return { success: true, share: newShare, message: 'Partage créé. Appliquez les modifications pour activer.' };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function updateShare(id, updates) {
  try {
    const config = await loadConfig();
    const shareIndex = config.shares.findIndex(s => s.id === id);

    if (shareIndex === -1) {
      return { success: false, error: 'Partage non trouvé' };
    }

    const allowedUpdates = ['comment', 'browseable', 'writable', 'guestOk', 'validUsers', 'writeList', 'createMask', 'directoryMask', 'enabled', 'path'];

    for (const key of Object.keys(updates)) {
      if (allowedUpdates.includes(key)) {
        config.shares[shareIndex][key] = updates[key];
      }
    }

    await saveConfigFile(config);

    return { success: true, share: config.shares[shareIndex], message: 'Partage modifié. Appliquez les modifications pour activer.' };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function deleteShare(id) {
  try {
    const config = await loadConfig();
    const shareIndex = config.shares.findIndex(s => s.id === id);

    if (shareIndex === -1) {
      return { success: false, error: 'Partage non trouvé' };
    }

    const deletedShare = config.shares.splice(shareIndex, 1)[0];
    await saveConfigFile(config);

    return { success: true, message: 'Partage supprimé. Appliquez les modifications pour finaliser.', share: deletedShare };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function toggleShare(id, enabled) {
  try {
    return await updateShare(id, { enabled: !!enabled });
  } catch (error) {
    return { success: false, error: error.message };
  }
}

// ========== smb.conf Generation ==========

function generateGlobalSection(globalConfig) {
  const lines = ['[global]'];

  const mapping = {
    workgroup: 'workgroup',
    serverString: 'server string',
    security: 'security',
    mapToGuest: 'map to guest',
    logFile: 'log file',
    maxLogSize: 'max log size'
  };

  for (const [key, confKey] of Object.entries(mapping)) {
    if (globalConfig[key] !== undefined) {
      lines.push(`   ${confKey} = ${globalConfig[key]}`);
    }
  }

  // Add common settings
  lines.push('   dns proxy = no');
  lines.push('   server role = standalone server');
  lines.push('   obey pam restrictions = yes');
  lines.push('   unix password sync = yes');
  lines.push('   passwd program = /usr/bin/passwd %u');
  lines.push('   passwd chat = *Enter\\snew\\s*\\spassword:* %n\\n *Retype\\snew\\s*\\spassword:* %n\\n *password\\supdated\\ssuccessfully* .');
  lines.push('   pam password change = yes');

  return lines.join('\n');
}

function generateShareSection(share) {
  if (!share.enabled) return '';

  const lines = [`[${share.name}]`];

  if (share.comment) lines.push(`   comment = ${share.comment}`);
  lines.push(`   path = ${share.path}`);
  lines.push(`   browseable = ${share.browseable ? 'yes' : 'no'}`);
  lines.push(`   writable = ${share.writable ? 'yes' : 'no'}`);
  lines.push(`   guest ok = ${share.guestOk ? 'yes' : 'no'}`);

  if (share.validUsers && share.validUsers.length > 0) {
    lines.push(`   valid users = ${share.validUsers.join(' ')}`);
  }

  if (share.writeList && share.writeList.length > 0) {
    lines.push(`   write list = ${share.writeList.join(' ')}`);
  }

  lines.push(`   create mask = ${share.createMask}`);
  lines.push(`   directory mask = ${share.directoryMask}`);

  return lines.join('\n');
}

export async function generateSmbConf() {
  try {
    const config = await loadConfig();

    const header = `# Samba configuration file
# Generated by Server Dashboard - ${new Date().toISOString()}
# DO NOT EDIT MANUALLY - Changes will be overwritten

`;

    const globalSection = generateGlobalSection(config.globalConfig);
    const shareSections = config.shares
      .filter(s => s.enabled)
      .map(s => generateShareSection(s))
      .filter(s => s)
      .join('\n\n');

    const smbConf = header + globalSection + '\n\n' + shareSections + '\n';

    return { success: true, content: smbConf };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function applySmbConf() {
  try {
    const { SMB_CONF_PATH, SMB_CONF_BACKUP } = getEnv();

    // Generate new config
    const result = await generateSmbConf();
    if (!result.success) {
      return result;
    }

    // Backup existing config
    if (existsSync(SMB_CONF_PATH)) {
      await copyFile(SMB_CONF_PATH, SMB_CONF_BACKUP);
    }

    // Write new config
    await writeFile(SMB_CONF_PATH, result.content);

    // Test config with testparm
    const testResult = await testSmbConf();
    if (!testResult.success) {
      // Restore backup if test fails
      if (existsSync(SMB_CONF_BACKUP)) {
        await copyFile(SMB_CONF_BACKUP, SMB_CONF_PATH);
      }
      return { success: false, error: `Configuration invalide: ${testResult.error}` };
    }

    // Reload Samba
    const reloadResult = await reloadSamba();
    if (!reloadResult.success) {
      return { success: false, error: `Configuration écrite mais rechargement échoué: ${reloadResult.error}` };
    }

    return { success: true, message: 'Configuration appliquée et Samba rechargé' };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function testSmbConf() {
  try {
    const { stdout, stderr } = await execAsync('testparm -s 2>&1', { timeout: 10000 });

    // testparm outputs to stderr for warnings, check for actual errors
    const output = stdout + stderr;
    if (output.includes('Error') || output.includes('Unknown parameter')) {
      return { success: false, error: output };
    }

    return { success: true, message: 'Configuration valide', output };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

// ========== Service Control ==========

export async function getServiceStatus() {
  try {
    const services = ['smbd', 'nmbd'];
    const status = {};

    for (const service of services) {
      try {
        const { stdout } = await execAsync(`systemctl is-active ${service} 2>/dev/null`);
        status[service] = {
          active: stdout.trim() === 'active',
          status: stdout.trim()
        };
      } catch {
        status[service] = {
          active: false,
          status: 'inactive'
        };
      }
    }

    // Get more details if active
    for (const service of services) {
      if (status[service].active) {
        try {
          const { stdout } = await execAsync(`systemctl show ${service} --property=MainPID,ActiveEnterTimestamp --no-pager`);
          const lines = stdout.split('\n');
          for (const line of lines) {
            const [key, value] = line.split('=');
            if (key === 'MainPID') status[service].pid = parseInt(value) || null;
            if (key === 'ActiveEnterTimestamp') status[service].startedAt = value || null;
          }
        } catch {
          // Ignore errors for additional details
        }
      }
    }

    return { success: true, status };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function restartSamba() {
  try {
    await execAsync('sudo systemctl restart smbd nmbd', { timeout: 30000 });
    return { success: true, message: 'Services Samba redémarrés' };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function reloadSamba() {
  try {
    await execAsync('sudo systemctl reload smbd', { timeout: 10000 });
    return { success: true, message: 'Configuration Samba rechargée' };
  } catch (error) {
    // If reload fails, try restart
    try {
      await execAsync('sudo systemctl restart smbd nmbd', { timeout: 30000 });
      return { success: true, message: 'Services Samba redémarrés (reload non supporté)' };
    } catch (restartError) {
      return { success: false, error: restartError.message };
    }
  }
}

// ========== Session Monitoring ==========

export async function getActiveSessions() {
  try {
    const { stdout } = await execAsync('smbstatus -b 2>/dev/null || echo ""', { timeout: 10000 });

    if (!stdout.trim()) {
      return { success: true, sessions: [] };
    }

    const sessions = [];
    const lines = stdout.split('\n');
    let inSessionSection = false;

    for (const line of lines) {
      // Skip header lines
      if (line.includes('PID') && line.includes('Username')) {
        inSessionSection = true;
        continue;
      }
      if (line.startsWith('---')) continue;
      if (!inSessionSection) continue;
      if (!line.trim()) continue;

      // Parse session line: PID  Username  Group  Machine  Protocol  Version  Encryption  Signing
      const parts = line.trim().split(/\s+/);
      if (parts.length >= 4) {
        sessions.push({
          pid: parseInt(parts[0]) || 0,
          username: parts[1] || 'unknown',
          group: parts[2] || '',
          machine: parts[3] || '',
          protocol: parts[4] || '',
          encryption: parts[5] || '',
          signing: parts[6] || ''
        });
      }
    }

    return { success: true, sessions };
  } catch (error) {
    return { success: false, error: error.message, sessions: [] };
  }
}

export async function getOpenFiles() {
  try {
    const { stdout } = await execAsync('smbstatus -L 2>/dev/null || echo ""', { timeout: 10000 });

    if (!stdout.trim()) {
      return { success: true, files: [] };
    }

    const files = [];
    const lines = stdout.split('\n');
    let inFilesSection = false;

    for (const line of lines) {
      // Skip header lines
      if (line.includes('Pid') && line.includes('Uid')) {
        inFilesSection = true;
        continue;
      }
      if (line.startsWith('---')) continue;
      if (!inFilesSection) continue;
      if (!line.trim()) continue;

      // Parse file line
      const parts = line.trim().split(/\s+/);
      if (parts.length >= 5) {
        // Last part is the filename (may contain spaces, so join remaining parts)
        const filename = parts.slice(6).join(' ') || parts[parts.length - 1];
        files.push({
          pid: parseInt(parts[0]) || 0,
          uid: parseInt(parts[1]) || 0,
          denyMode: parts[2] || '',
          access: parts[3] || '',
          rwAccess: parts[4] || '',
          sharePath: parts[5] || '',
          name: filename
        });
      }
    }

    return { success: true, files };
  } catch (error) {
    return { success: false, error: error.message, files: [] };
  }
}

export async function getShareConnections(shareName) {
  try {
    const { stdout } = await execAsync('smbstatus -S 2>/dev/null || echo ""', { timeout: 10000 });

    if (!stdout.trim()) {
      return { success: true, connections: [] };
    }

    const connections = [];
    const lines = stdout.split('\n');
    let inShareSection = false;

    for (const line of lines) {
      if (line.includes('Service') && line.includes('pid')) {
        inShareSection = true;
        continue;
      }
      if (line.startsWith('---')) continue;
      if (!inShareSection) continue;
      if (!line.trim()) continue;

      const parts = line.trim().split(/\s+/);
      if (parts.length >= 4) {
        const service = parts[0];
        // Filter by share name if provided
        if (!shareName || service.toLowerCase() === shareName.toLowerCase()) {
          connections.push({
            service: service,
            pid: parseInt(parts[1]) || 0,
            machine: parts[2] || '',
            connectedAt: parts.slice(3).join(' ') || ''
          });
        }
      }
    }

    return { success: true, connections };
  } catch (error) {
    return { success: false, error: error.message, connections: [] };
  }
}

// ========== User Management ==========

export async function getUsers() {
  try {
    const { stdout } = await execAsync('pdbedit -L -v 2>/dev/null || echo ""', { timeout: 10000 });

    if (!stdout.trim()) {
      return { success: true, users: [] };
    }

    const users = [];
    let currentUser = null;

    const lines = stdout.split('\n');
    for (const line of lines) {
      if (line.startsWith('---------------')) {
        if (currentUser && currentUser.username) {
          users.push(currentUser);
        }
        currentUser = {};
        continue;
      }

      if (!currentUser) continue;

      const colonIndex = line.indexOf(':');
      if (colonIndex === -1) continue;

      const key = line.substring(0, colonIndex).trim();
      const value = line.substring(colonIndex + 1).trim();

      switch (key) {
        case 'Unix username':
          currentUser.username = value;
          break;
        case 'NT username':
          currentUser.ntUsername = value;
          break;
        case 'Full Name':
          currentUser.fullName = value;
          break;
        case 'Account Flags':
          currentUser.flags = value;
          currentUser.enabled = !value.includes('D');
          break;
        case 'User SID':
          currentUser.sid = value;
          break;
      }
    }

    // Don't forget the last user
    if (currentUser && currentUser.username) {
      users.push(currentUser);
    }

    return { success: true, users };
  } catch (error) {
    return { success: false, error: error.message, users: [] };
  }
}

export async function addUser(username, password) {
  try {
    // Validate username
    const usernameRegex = /^[a-z_][a-z0-9_-]*$/;
    if (!usernameRegex.test(username)) {
      return { success: false, error: 'Nom d\'utilisateur invalide. Utilisez uniquement des lettres minuscules, chiffres, tirets et underscores.' };
    }

    if (!password || password.length < 8) {
      return { success: false, error: 'Le mot de passe doit contenir au moins 8 caractères' };
    }

    // Check if system user exists
    try {
      await execAsync(`id ${username}`);
    } catch {
      return { success: false, error: `L'utilisateur système "${username}" n'existe pas. Créez-le d'abord avec useradd.` };
    }

    // Add Samba user using expect-like approach
    return new Promise((resolve) => {
      const smbpasswd = spawn('sudo', ['smbpasswd', '-a', username], {
        stdio: ['pipe', 'pipe', 'pipe']
      });

      let output = '';
      let errorOutput = '';

      smbpasswd.stdout.on('data', (data) => {
        output += data.toString();
      });

      smbpasswd.stderr.on('data', (data) => {
        const text = data.toString();
        errorOutput += text;

        // Respond to password prompts
        if (text.includes('password:')) {
          smbpasswd.stdin.write(password + '\n');
        }
      });

      smbpasswd.on('close', (code) => {
        if (code === 0) {
          resolve({ success: true, message: `Utilisateur Samba "${username}" créé` });
        } else {
          resolve({ success: false, error: errorOutput || 'Échec de la création de l\'utilisateur' });
        }
      });

      smbpasswd.on('error', (err) => {
        resolve({ success: false, error: err.message });
      });

      // Timeout
      setTimeout(() => {
        smbpasswd.kill();
        resolve({ success: false, error: 'Timeout lors de la création de l\'utilisateur' });
      }, 30000);
    });
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function removeUser(username) {
  try {
    await execAsync(`sudo smbpasswd -x ${username}`, { timeout: 10000 });
    return { success: true, message: `Utilisateur Samba "${username}" supprimé` };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function changePassword(username, password) {
  try {
    if (!password || password.length < 8) {
      return { success: false, error: 'Le mot de passe doit contenir au moins 8 caractères' };
    }

    return new Promise((resolve) => {
      const smbpasswd = spawn('sudo', ['smbpasswd', username], {
        stdio: ['pipe', 'pipe', 'pipe']
      });

      let errorOutput = '';

      smbpasswd.stderr.on('data', (data) => {
        const text = data.toString();
        errorOutput += text;

        if (text.includes('password:')) {
          smbpasswd.stdin.write(password + '\n');
        }
      });

      smbpasswd.on('close', (code) => {
        if (code === 0) {
          resolve({ success: true, message: 'Mot de passe modifié' });
        } else {
          resolve({ success: false, error: errorOutput || 'Échec de la modification du mot de passe' });
        }
      });

      smbpasswd.on('error', (err) => {
        resolve({ success: false, error: err.message });
      });

      setTimeout(() => {
        smbpasswd.kill();
        resolve({ success: false, error: 'Timeout' });
      }, 30000);
    });
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function enableUser(username) {
  try {
    await execAsync(`sudo smbpasswd -e ${username}`, { timeout: 10000 });
    return { success: true, message: `Utilisateur "${username}" activé` };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function disableUser(username) {
  try {
    await execAsync(`sudo smbpasswd -d ${username}`, { timeout: 10000 });
    return { success: true, message: `Utilisateur "${username}" désactivé` };
  } catch (error) {
    return { success: false, error: error.message };
  }
}
