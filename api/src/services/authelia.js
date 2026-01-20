import { readFile, writeFile, mkdir } from 'fs/promises';
import { existsSync } from 'fs';
import path from 'path';
import http from 'http';

// Environment configuration
const getEnv = () => ({
  AUTH_SERVICE_URL: process.env.AUTH_SERVICE_URL || 'http://localhost:9100',
  AUTH_DATA_DIR: process.env.AUTH_DATA_DIR || '/ssd_pool/auth-service/data'
});

// Groupes prédéfinis avec leurs descriptions
const PREDEFINED_GROUPS = {
  admins: {
    name: 'admins',
    displayName: 'Administrateurs',
    description: 'Accès complet, gestion des utilisateurs et services'
  },
  users: {
    name: 'users',
    displayName: 'Utilisateurs',
    description: 'Accès basique aux services'
  }
};

// ========== HTTP Client pour auth-service ==========

async function authServiceRequest(method, endpoint, data = null) {
  const { AUTH_SERVICE_URL } = getEnv();
  const url = new URL(endpoint, AUTH_SERVICE_URL);

  return new Promise((resolve, reject) => {
    const options = {
      hostname: url.hostname,
      port: url.port || 9100,
      path: url.pathname,
      method,
      headers: { 'Content-Type': 'application/json' }
    };

    const req = http.request(options, (res) => {
      let body = '';
      res.on('data', chunk => body += chunk);
      res.on('end', () => {
        try {
          const result = body ? JSON.parse(body) : {};
          resolve(result);
        } catch {
          resolve({ success: false, error: 'Invalid response' });
        }
      });
    });

    req.on('error', (err) => {
      resolve({ success: false, error: `Connection failed: ${err.message}` });
    });

    req.setTimeout(5000, () => {
      req.destroy();
      resolve({ success: false, error: 'Request timeout' });
    });

    if (data) req.write(JSON.stringify(data));
    req.end();
  });
}

// ========== Status Auth Service ==========

export async function getAutheliaStatus() {
  const { AUTH_SERVICE_URL, AUTH_DATA_DIR } = getEnv();

  try {
    const result = await authServiceRequest('GET', '/api/health');

    if (result.status === 'ok') {
      const usersFileExists = existsSync(path.join(AUTH_DATA_DIR, 'users.yml'));
      const dbExists = existsSync(path.join(AUTH_DATA_DIR, 'auth.db'));

      return {
        success: true,
        status: 'running',
        healthy: true,
        configExists: usersFileExists,
        usersExists: usersFileExists,
        dbExists,
        url: AUTH_SERVICE_URL,
        service: 'auth-service'
      };
    }

    return {
      success: true,
      status: 'unhealthy',
      healthy: false,
      url: AUTH_SERVICE_URL
    };
  } catch (error) {
    const usersFileExists = existsSync(path.join(AUTH_DATA_DIR, 'users.yml'));

    if (usersFileExists) {
      return {
        success: true,
        status: 'stopped',
        healthy: false,
        configExists: true,
        message: 'Le service d\'authentification est configuré mais ne répond pas'
      };
    }

    return {
      success: true,
      status: 'not_installed',
      healthy: false,
      configExists: false,
      message: 'Le service d\'authentification n\'est pas installé ou configuré'
    };
  }
}

// ========== Gestion Users via Auth Service API ==========

// Charger les utilisateurs directement depuis le fichier YAML
import yaml from 'js-yaml';

async function loadUsersDatabase() {
  const { AUTH_DATA_DIR } = getEnv();
  const usersFile = path.join(AUTH_DATA_DIR, 'users.yml');

  if (!existsSync(usersFile)) {
    return { users: {} };
  }

  try {
    const content = await readFile(usersFile, 'utf-8');
    const data = yaml.load(content);
    return data || { users: {} };
  } catch (error) {
    console.error('Erreur loadUsersDatabase:', error.message);
    return { users: {} };
  }
}

async function saveUsersDatabase(data) {
  const { AUTH_DATA_DIR } = getEnv();
  const usersFile = path.join(AUTH_DATA_DIR, 'users.yml');

  if (!existsSync(AUTH_DATA_DIR)) {
    await mkdir(AUTH_DATA_DIR, { recursive: true });
  }

  const yamlContent = yaml.dump(data, {
    indent: 2,
    lineWidth: -1,
    noRefs: true
  });

  await writeFile(usersFile, yamlContent, 'utf-8');
}

// Hash password en appelant auth-service (ou utiliser argon2 directement)
import argon2 from 'argon2';

async function hashPassword(password) {
  return argon2.hash(password, {
    type: argon2.argon2id,
    memoryCost: 65536,
    timeCost: 3,
    parallelism: 4
  });
}

// ========== CRUD Users ==========

export async function getUsers() {
  try {
    const db = await loadUsersDatabase();
    const users = [];

    for (const [username, userData] of Object.entries(db.users || {})) {
      users.push({
        username,
        displayname: userData.displayname || username,
        email: userData.email || '',
        groups: userData.groups || [],
        disabled: userData.disabled || false
      });
    }

    return { success: true, users };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function getUser(username) {
  try {
    const db = await loadUsersDatabase();
    const userData = db.users?.[username];

    if (!userData) {
      return { success: false, error: 'Utilisateur non trouvé' };
    }

    return {
      success: true,
      user: {
        username,
        displayname: userData.displayname || username,
        email: userData.email || '',
        groups: userData.groups || [],
        disabled: userData.disabled || false
      }
    };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function createUser(username, password, displayname, email, groups = ['users']) {
  try {
    // Validation
    if (!username || username.length < 3) {
      return { success: false, error: 'Le nom d\'utilisateur doit contenir au moins 3 caractères' };
    }

    if (!password || password.length < 8) {
      return { success: false, error: 'Le mot de passe doit contenir au moins 8 caractères' };
    }

    // Valider le format username (alphanumeric + underscore + dash)
    if (!/^[a-zA-Z0-9_-]+$/.test(username)) {
      return { success: false, error: 'Le nom d\'utilisateur ne peut contenir que des lettres, chiffres, underscores et tirets' };
    }

    // Valider les groupes
    const validGroups = groups.filter(g => PREDEFINED_GROUPS[g]);
    if (validGroups.length === 0) {
      validGroups.push('users');
    }

    const db = await loadUsersDatabase();

    // Vérifier doublon
    if (db.users?.[username.toLowerCase()]) {
      return { success: false, error: 'Cet utilisateur existe déjà' };
    }

    // Hash du mot de passe
    const passwordHash = await hashPassword(password);

    // Créer l'utilisateur
    if (!db.users) db.users = {};

    db.users[username.toLowerCase()] = {
      disabled: false,
      displayname: displayname || username,
      email: email || `${username}@localhost`,
      password: passwordHash,
      groups: validGroups,
      created: new Date().toISOString()
    };

    await saveUsersDatabase(db);

    return {
      success: true,
      user: {
        username: username.toLowerCase(),
        displayname: displayname || username,
        email: email || `${username}@localhost`,
        groups: validGroups,
        disabled: false
      }
    };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function updateUser(username, updates) {
  try {
    const db = await loadUsersDatabase();
    const userData = db.users?.[username.toLowerCase()];

    if (!userData) {
      return { success: false, error: 'Utilisateur non trouvé' };
    }

    // Mettre à jour les champs autorisés
    if (updates.displayname !== undefined) {
      userData.displayname = updates.displayname;
    }

    if (updates.email !== undefined) {
      userData.email = updates.email;
    }

    if (updates.groups !== undefined) {
      const validGroups = updates.groups.filter(g => PREDEFINED_GROUPS[g]);
      if (validGroups.length > 0) {
        userData.groups = validGroups;
      }
    }

    if (updates.disabled !== undefined) {
      userData.disabled = !!updates.disabled;
    }

    db.users[username.toLowerCase()] = userData;
    await saveUsersDatabase(db);

    return {
      success: true,
      user: {
        username: username.toLowerCase(),
        displayname: userData.displayname,
        email: userData.email,
        groups: userData.groups,
        disabled: userData.disabled
      }
    };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function changePassword(username, newPassword) {
  try {
    if (!newPassword || newPassword.length < 8) {
      return { success: false, error: 'Le mot de passe doit contenir au moins 8 caractères' };
    }

    const db = await loadUsersDatabase();
    const userData = db.users?.[username.toLowerCase()];

    if (!userData) {
      return { success: false, error: 'Utilisateur non trouvé' };
    }

    userData.password = await hashPassword(newPassword);

    db.users[username.toLowerCase()] = userData;
    await saveUsersDatabase(db);

    return { success: true, message: 'Mot de passe modifié avec succès' };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function deleteUser(username) {
  try {
    const db = await loadUsersDatabase();

    if (!db.users?.[username.toLowerCase()]) {
      return { success: false, error: 'Utilisateur non trouvé' };
    }

    // Empêcher la suppression du dernier admin
    const admins = Object.entries(db.users || {}).filter(
      ([, u]) => u.groups?.includes('admins') && !u.disabled
    );

    const userToDelete = db.users[username.toLowerCase()];
    if (userToDelete.groups?.includes('admins') && admins.length <= 1) {
      return { success: false, error: 'Impossible de supprimer le dernier administrateur' };
    }

    delete db.users[username.toLowerCase()];
    await saveUsersDatabase(db);

    return { success: true, message: 'Utilisateur supprimé' };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

// ========== Groupes ==========

export async function getGroups() {
  try {
    const db = await loadUsersDatabase();

    // Compter les membres de chaque groupe
    const groupCounts = {};
    for (const group of Object.keys(PREDEFINED_GROUPS)) {
      groupCounts[group] = 0;
    }

    for (const userData of Object.values(db.users || {})) {
      for (const group of userData.groups || []) {
        if (groupCounts[group] !== undefined) {
          groupCounts[group]++;
        }
      }
    }

    const groups = Object.values(PREDEFINED_GROUPS).map(group => ({
      ...group,
      memberCount: groupCounts[group.name] || 0
    }));

    return { success: true, groups };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

// ========== Bootstrap ==========

export async function bootstrapAdmin(password) {
  try {
    const db = await loadUsersDatabase();

    // Vérifier si un admin existe déjà
    const existingAdmins = Object.entries(db.users || {}).filter(
      ([, u]) => u.groups?.includes('admins')
    );

    if (existingAdmins.length > 0) {
      return { success: false, error: 'Un administrateur existe déjà' };
    }

    // Créer l'admin par défaut
    const result = await createUser(
      'admin',
      password,
      'Administrateur',
      'admin@mynetwk.biz',
      ['admins']
    );

    return result;
  } catch (error) {
    return { success: false, error: error.message };
  }
}

// ========== Installation Instructions ==========

export function getInstallationInstructions() {
  return {
    success: true,
    instructions: `# Service d'authentification

## Démarrage du service

\`\`\`bash
# Démarrer avec PM2
cd /ssd_pool/auth-service
pm2 start ecosystem.config.cjs

# Ou manuellement pour le développement
npm run dev
\`\`\`

## Vérification

\`\`\`bash
curl http://localhost:9100/api/health
\`\`\`

## Configuration

Le service utilise:
- Port: 9100
- Base de données: /ssd_pool/auth-service/data/auth.db
- Utilisateurs: /ssd_pool/auth-service/data/users.yml

## Portail utilisateur

Accédez à https://auth.mynetwk.biz pour:
- Se connecter
- Gérer son compte
`
  };
}
