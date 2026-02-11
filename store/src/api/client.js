import * as SecureStore from 'expo-secure-store';

const SERVER_KEY = 'server_url';
let _baseUrl = null;

export async function getServerUrl() {
  if (_baseUrl) return _baseUrl;
  const stored = await SecureStore.getItemAsync(SERVER_KEY);
  _baseUrl = stored || null;
  return _baseUrl;
}

export async function setServerUrl(url) {
  const clean = url.replace(/\/+$/, '');
  await SecureStore.setItemAsync(SERVER_KEY, clean);
  _baseUrl = clean;
}

async function apiFetch(path, options = {}) {
  const base = await getServerUrl();
  if (!base) throw new Error('Serveur non configure');
  const res = await fetch(`${base}/api${path}`, {
    ...options,
    headers: { 'Content-Type': 'application/json', ...options.headers },
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error || `HTTP ${res.status}`);
  }
  return res.json();
}

export const getStoreApps = () => apiFetch('/store/apps');

export const getStoreApp = (slug) => apiFetch(`/store/apps/${slug}`);

export const checkStoreUpdates = (installed) => {
  const param = installed.map(i => `${i.slug}:${i.version}`).join(',');
  return apiFetch(`/store/updates?installed=${param}`);
};

export function getDownloadUrl(slug, version) {
  return `${_baseUrl}/api/store/releases/${slug}/${version}/download`;
}
