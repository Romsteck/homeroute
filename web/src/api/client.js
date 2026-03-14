import axios from 'axios';

const api = axios.create({
  baseURL: '/api',
  timeout: 30000,
  withCredentials: true  // Enable cookies for session-based auth
});

// Interceptor to handle session expiration
api.interceptors.response.use(
  (response) => {
    // Check if response indicates session expired
    if (response.data && response.data.success === false && response.data.error === 'Session expiree') {
      // Force cookie deletion by setting it to expire immediately
      document.cookie = 'auth_session=; path=/; expires=Thu, 01 Jan 1970 00:00:00 UTC; domain=' + window.location.hostname;
      document.cookie = 'auth_session=; path=/; expires=Thu, 01 Jan 1970 00:00:00 UTC';
    }
    return response;
  },
  (error) => {
    // Handle 401 errors
    if (error.response && error.response.status === 401) {
      // Force cookie deletion
      document.cookie = 'auth_session=; path=/; expires=Thu, 01 Jan 1970 00:00:00 UTC; domain=' + window.location.hostname;
      document.cookie = 'auth_session=; path=/; expires=Thu, 01 Jan 1970 00:00:00 UTC';
    }
    return Promise.reject(error);
  }
);

// Services Status
export const getServicesStatus = () => api.get('/services/status');

// DNS/DHCP
export const getDnsConfig = () => api.get('/dns-dhcp/config');
export const getDhcpLeases = () => api.get('/dns-dhcp/leases');

// AdBlock
export const getAdblockStats = () => api.get('/adblock/stats');
export const getWhitelist = () => api.get('/adblock/whitelist');
export const addToWhitelist = (domain) => api.post('/adblock/whitelist', { domain });
export const removeFromWhitelist = (domain) => api.delete(`/adblock/whitelist/${domain}`);
export const updateAdblockLists = () => api.post('/adblock/update');
export const searchBlocked = (query) => api.get('/adblock/search', { params: { q: query } });

// DDNS
export const getDdnsStatus = () => api.get('/ddns/status');
export const forceDdnsUpdate = () => api.post('/ddns/update');
export const updateDdnsToken = (token) => api.put('/ddns/token', { token });
export const updateDdnsConfig = (config) => api.put('/ddns/config', config);

// Reverse Proxy
export const getReverseProxyConfig = () => api.get('/reverseproxy/config');
export const getReverseProxyStatus = () => api.get('/reverseproxy/status');
export const getReverseProxyHosts = () => api.get('/reverseproxy/hosts');
export const addReverseProxyHost = (host) => api.post('/reverseproxy/hosts', host);
export const updateReverseProxyHost = (id, updates) => api.put(`/reverseproxy/hosts/${id}`, updates);
export const deleteReverseProxyHost = (id) => api.delete(`/reverseproxy/hosts/${id}`);
export const toggleReverseProxyHost = (id, enabled) => api.post(`/reverseproxy/hosts/${id}/toggle`, { enabled });
export const updateBaseDomain = (baseDomain) => api.put('/reverseproxy/config/domain', { baseDomain });
export const renewCertificates = () => api.post('/reverseproxy/certificates/renew');
export const reloadProxy = () => api.post('/reverseproxy/reload');
export const getCertificatesStatus = () => api.get('/reverseproxy/certificates/status');

// Rust Proxy
export const getRustProxyStatus = () => api.get('/rust-proxy/status');

// Auth - Session (login page)
export const login = (code, remember_me = false) => api.post('/auth/login', { code, remember_me });
export const logout = () => api.post('/auth/logout');
export const getMe = () => api.get('/auth/me');
export const changeCode = (new_code) => api.post('/auth/change-code', { new_code });

// System Updates
export const getUpdatesStatus = () => api.get('/updates/status');
export const getLastUpdatesCheck = () => api.get('/updates/last');
export const checkForUpdates = () => api.post('/updates/check', {}, { timeout: 300000 });
export const cancelUpdatesCheck = () => api.post('/updates/cancel');

// System Updates - Upgrade actions
export const getUpgradeStatus = () => api.get('/updates/upgrade/status');
export const runAptUpgrade = () => api.post('/updates/upgrade/apt', {}, { timeout: 1800000 });
export const runAptFullUpgrade = () => api.post('/updates/upgrade/apt-full', {}, { timeout: 1800000 });
export const runSnapRefresh = () => api.post('/updates/upgrade/snap', {}, { timeout: 1800000 });
export const cancelUpgrade = () => api.post('/updates/upgrade/cancel');

// Energy - CPU Info
export const getCpuInfo = () => api.get('/energy/cpu');

// Energy - Modes (unified: economy/auto/performance)
export const getCurrentEnergyMode = () => api.get('/energy/mode');
export const setEnergyMode = (mode) => api.post(`/energy/mode/${mode}`);

// Energy - Schedule
export const getEnergySchedule = () => api.get('/energy/schedule');
export const saveEnergySchedule = (config) => api.post('/energy/schedule', config);

// Energy - Auto-select
export const getAutoSelectConfig = () => api.get('/energy/autoselect');
export const saveAutoSelectConfig = (config) => api.post('/energy/autoselect', config);
export const getSelectableInterfaces = () => api.get('/energy/interfaces');

// Energy - Benchmark
export const getBenchmarkStatus = () => api.get('/energy/benchmark');
export const startBenchmark = (duration = 60) => api.post('/energy/benchmark/start', { duration });
export const stopBenchmark = () => api.post('/energy/benchmark/stop');


export default api;

// ========== Hosts (unified servers + WoL) ==========

export const getHosts = () => api.get('/hosts');
export const addHost = (data) => api.post('/hosts', data);
export const updateHost = (id, data) => api.put(`/hosts/${id}`, data);
export const deleteHost = (id) => api.delete(`/hosts/${id}`);
export const testHostConnection = (id) => api.post(`/hosts/${id}/test`);
// Hosts - Power actions
export const wakeHost = (id) => api.post(`/hosts/${id}/wake`);
export const shutdownHost = (id) => api.post(`/hosts/${id}/shutdown`);
export const rebootHost = (id) => api.post(`/hosts/${id}/reboot`);
export const sleepHost = (id) => api.post(`/hosts/${id}/sleep`);
export const setWolMac = (id, mac) => api.post(`/hosts/${id}/wol-mac`, { mac });
export const setAutoOff = (id, mode, minutes) => api.post(`/hosts/${id}/auto-off`, { mode, minutes });
export const setHostRole = (id, role) => api.put(`/hosts/${id}/role`, { role });
export const updateHostAgents = () => api.post('/hosts/agents/update');
export const updateLocalHostConfig = (data) => api.put('/hosts/local/config', data);
export const getLocalInterfaces = () => api.get('/hosts/local/interfaces');

// Cloud Relay
export const getCloudRelayStatus = () => api.get('/cloud-relay/status');
export const enableCloudRelay = () => api.post('/cloud-relay/enable');
export const disableCloudRelay = () => api.post('/cloud-relay/disable');
export const bootstrapCloudRelay = (data) => api.post('/cloud-relay/bootstrap', data, { timeout: 300000 });
export const updateCloudRelayConfig = (config) => api.put('/cloud-relay/config', config);
export const pushCloudRelayUpdate = () => api.post('/cloud-relay/update', {}, { timeout: 120000 });

// Edge Stats
export const getEdgeStats = () => api.get('/edge/stats');

// Containers (nspawn)
export const getContainers = () => api.get('/containers');
export const createContainer = (data) => api.post('/containers', data);
export const updateContainer = (id, data) => api.put(`/containers/${id}`, data);
export const deleteContainer = (id) => api.delete(`/containers/${id}`);
export const startContainer = (id) => api.post(`/containers/${id}/start`);
export const stopContainer = (id) => api.post(`/containers/${id}/stop`);
export const migrateContainer = (id, targetHostId) => api.post(`/containers/${id}/migrate`, { target_host_id: targetHostId });
export const getMigrationStatus = (id) => api.get(`/containers/${id}/migrate/status`);
export const cancelMigration = (id) => api.post(`/containers/${id}/migrate/cancel`);
export const getContainersConfig = () => api.get('/containers/config');
export const updateContainersConfig = (data) => api.put('/containers/config', data);
export const renameContainer = (id, data) => api.post(`/containers/${id}/rename`, data);
export const getRenameStatus = (id) => api.get(`/containers/${id}/rename/status`);

// Prod status/logs (queried via dev container's linked prod)
export const getProdStatus = (devAppId) => api.get(`/applications/${devAppId}/prod/status`);
export const getProdLogs = (devAppId, lines = 50) => api.get(`/applications/${devAppId}/prod/logs`, { params: { lines } });

// Dataverse
export const getDataverseOverview = () => api.get('/dataverse/overview');
export const getDataverseTable = (appId, tableName) => api.get(`/dataverse/apps/${appId}/tables/${tableName}`);
export const getDataverseRelations = (appId) => api.get(`/dataverse/apps/${appId}/relations`);
export const getDataverseRows = (appId, table, params) => api.get(`/dataverse/apps/${appId}/tables/${table}/rows`, { params });
export const insertDataverseRows = (appId, table, rows) => api.post(`/dataverse/apps/${appId}/tables/${table}/rows`, { rows });
export const updateDataverseRows = (appId, table, data) => api.put(`/dataverse/apps/${appId}/tables/${table}/rows`, data);
export const deleteDataverseRows = (appId, table, filters) => api.delete(`/dataverse/apps/${appId}/tables/${table}/rows`, { data: { filters } });

// Store
export const getStoreApps = () => api.get('/store/apps');

// ========== Git ==========
export const getGitRepos = () => api.get('/git/repos');
export const getGitRepo = (slug) => api.get(`/git/repos/${slug}`);
export const getGitCommits = (slug, limit = 50) => api.get(`/git/repos/${slug}/commits`, { params: { limit } });
export const getGitBranches = (slug) => api.get(`/git/repos/${slug}/branches`);
export const triggerGitMirrorSync = (slug) => api.post(`/git/repos/${slug}/mirror/sync`);
export const syncAllGitRepos = () => api.post('/git/repos/sync-all');
export const getGitSshKey = () => api.get('/git/ssh-key');
export const generateGitSshKey = () => api.post('/git/ssh-key');
export const getGitConfig = () => api.get('/git/config');
export const updateGitConfig = (config) => api.put('/git/config', config);
export const getStoreApp = (slug) => api.get(`/store/apps/${slug}`);
export const checkStoreUpdates = (installed) => {
  const param = installed.map(i => `${i.slug}:${i.version}`).join(',');
  return api.get(`/store/updates?installed=${param}`);
};
export const downloadStoreRelease = (slug, version) => {
  const a = document.createElement('a');
  a.href = `/api/store/releases/${slug}/${version}/download`;
  a.download = '';
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
};

// Unified Updates
export const scanAllUpdates = () => api.post('/updates/scan-all');
export const getScanResults = () => api.get('/updates/scan-all/results');
export const upgradeTarget = (targetId, category) =>
  api.post('/updates/upgrade-target', { target_id: targetId, category }, { timeout: 1800000 });
export const getUpdateHistory = (limit = 50) => api.get('/updates/history', { params: { limit } });

// ========== Backup ==========
export const getBackupStatus = () => api.get('/backup/status');
export const getBackupRepos = () => api.get('/backup/repos');
export const getBackupJobs = () => api.get('/backup/jobs');
export const triggerBackup = () => api.post('/backup/trigger');
