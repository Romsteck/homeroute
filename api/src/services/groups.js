/**
 * Service de gestion des groupes
 *
 * Gere les groupes built-in (admins, users) et les groupes personnalises.
 * Les groupes custom sont stockes dans /data/groups.json.
 */

import fs from 'fs';
import path from 'path';
import yaml from 'js-yaml';
import { getUsers } from './authUsers.js';

const DATA_DIR = process.env.AUTH_DATA_DIR || path.join(process.cwd(), 'data');
const GROUPS_FILE = path.join(DATA_DIR, 'groups.json');

// Groupes built-in (non supprimables, non modifiables)
const BUILTIN_GROUPS = {
  admins: {
    id: 'admins',
    name: 'Administrateurs',
    description: 'Acces complet, gestion des utilisateurs et services',
    color: '#EF4444',
    builtIn: true
  },
  users: {
    id: 'users',
    name: 'Utilisateurs',
    description: 'Acces basique aux services',
    color: '#3B82F6',
    builtIn: true
  }
};

// ========== Persistence ==========

function loadCustomGroups() {
  try {
    if (!fs.existsSync(GROUPS_FILE)) {
      return [];
    }
    const content = fs.readFileSync(GROUPS_FILE, 'utf8');
    const data = JSON.parse(content);
    return data.groups || [];
  } catch (error) {
    console.error('Error loading groups file:', error);
    return [];
  }
}

function saveCustomGroups(groups) {
  try {
    const dir = path.dirname(GROUPS_FILE);
    if (!fs.existsSync(dir)) {
      fs.mkdirSync(dir, { recursive: true });
    }
    fs.writeFileSync(GROUPS_FILE, JSON.stringify({ groups }, null, 2), 'utf8');
    return true;
  } catch (error) {
    console.error('Error saving groups file:', error);
    return false;
  }
}

// ========== Migration ==========

/**
 * Migre les utilisateurs ayant le groupe power_users vers users
 * A appeler au demarrage du service
 */
export function migratePowerUsers() {
  try {
    const usersFile = path.join(DATA_DIR, 'users.yml');
    if (!fs.existsSync(usersFile)) return;

    const content = fs.readFileSync(usersFile, 'utf8');
    const data = yaml.load(content) || { users: {} };

    let modified = false;
    for (const [username, userData] of Object.entries(data.users || {})) {
      if (userData.groups && userData.groups.includes('power_users')) {
        userData.groups = userData.groups.filter(g => g !== 'power_users');
        if (!userData.groups.includes('users')) {
          userData.groups.push('users');
        }
        modified = true;
        console.log(`Migrated user "${username}": removed power_users, ensured users group`);
      }
    }

    if (modified) {
      fs.writeFileSync(usersFile, yaml.dump(data, { lineWidth: -1 }), 'utf8');
      console.log('power_users migration complete');
    }
  } catch (error) {
    console.error('Error during power_users migration:', error);
  }
}

// ========== Public API ==========

/**
 * Retourne tous les groupes (built-in + custom) avec le nombre de membres
 */
export function getGroups() {
  const users = getUsers();
  const customGroups = loadCustomGroups();

  // Calculer les membres par groupe
  const memberCounts = {};
  for (const user of users) {
    for (const group of user.groups || []) {
      memberCounts[group] = (memberCounts[group] || 0) + 1;
    }
  }

  // Construire la liste
  const builtInList = Object.values(BUILTIN_GROUPS).map(g => ({
    ...g,
    memberCount: memberCounts[g.id] || 0
  }));

  const customList = customGroups.map(g => ({
    ...g,
    builtIn: false,
    memberCount: memberCounts[g.id] || 0
  }));

  return [...builtInList, ...customList];
}

/**
 * Retourne un groupe par son id
 */
export function getGroup(id) {
  if (BUILTIN_GROUPS[id]) {
    return { ...BUILTIN_GROUPS[id] };
  }
  const customGroups = loadCustomGroups();
  return customGroups.find(g => g.id === id) || null;
}

/**
 * Cree un nouveau groupe personnalise
 */
export function createGroup({ name, description, color }) {
  if (!name || typeof name !== 'string' || name.trim().length === 0) {
    return { success: false, error: 'Le nom du groupe est requis' };
  }

  // Generer l'id depuis le nom
  const id = name.toLowerCase()
    .normalize('NFD').replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '');

  if (!id || id.length < 2) {
    return { success: false, error: 'Le nom du groupe doit contenir au moins 2 caracteres alphanumeriques' };
  }

  // Verifier que l'id n'existe pas deja
  if (BUILTIN_GROUPS[id]) {
    return { success: false, error: 'Ce nom est reserve pour un groupe systeme' };
  }

  const customGroups = loadCustomGroups();
  if (customGroups.some(g => g.id === id)) {
    return { success: false, error: 'Un groupe avec ce nom existe deja' };
  }

  const newGroup = {
    id,
    name: name.trim(),
    description: (description || '').trim(),
    color: color || '#8B5CF6',
    createdAt: new Date().toISOString()
  };

  customGroups.push(newGroup);
  if (!saveCustomGroups(customGroups)) {
    return { success: false, error: 'Erreur lors de la sauvegarde' };
  }

  return { success: true, group: newGroup };
}

/**
 * Met a jour un groupe personnalise
 */
export function updateGroup(id, updates) {
  if (BUILTIN_GROUPS[id]) {
    return { success: false, error: 'Les groupes systeme ne peuvent pas etre modifies' };
  }

  const customGroups = loadCustomGroups();
  const index = customGroups.findIndex(g => g.id === id);

  if (index === -1) {
    return { success: false, error: 'Groupe non trouve' };
  }

  if (updates.name !== undefined) {
    customGroups[index].name = updates.name.trim();
  }
  if (updates.description !== undefined) {
    customGroups[index].description = updates.description.trim();
  }
  if (updates.color !== undefined) {
    customGroups[index].color = updates.color;
  }

  if (!saveCustomGroups(customGroups)) {
    return { success: false, error: 'Erreur lors de la sauvegarde' };
  }

  return { success: true, group: customGroups[index] };
}

/**
 * Supprime un groupe personnalise
 */
export function deleteGroup(id) {
  if (BUILTIN_GROUPS[id]) {
    return { success: false, error: 'Les groupes systeme ne peuvent pas etre supprimes' };
  }

  const customGroups = loadCustomGroups();
  const index = customGroups.findIndex(g => g.id === id);

  if (index === -1) {
    return { success: false, error: 'Groupe non trouve' };
  }

  // Verifier qu'aucun utilisateur n'est membre
  const users = getUsers();
  const members = users.filter(u => (u.groups || []).includes(id));
  if (members.length > 0) {
    return {
      success: false,
      error: `Impossible de supprimer : ${members.length} utilisateur(s) sont encore membres de ce groupe`
    };
  }

  customGroups.splice(index, 1);
  if (!saveCustomGroups(customGroups)) {
    return { success: false, error: 'Erreur lors de la sauvegarde' };
  }

  return { success: true };
}

/**
 * Verifie si un id de groupe est valide (built-in ou custom)
 */
export function isValidGroup(id) {
  if (BUILTIN_GROUPS[id]) return true;
  const customGroups = loadCustomGroups();
  return customGroups.some(g => g.id === id);
}

/**
 * Retourne la liste de tous les ids de groupes valides
 */
export function getAllGroupIds() {
  const customGroups = loadCustomGroups();
  return [
    ...Object.keys(BUILTIN_GROUPS),
    ...customGroups.map(g => g.id)
  ];
}
