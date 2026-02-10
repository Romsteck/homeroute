import { useState, useEffect, useRef, useCallback } from 'react';
import {
  Container,
  Plus,
  Trash2,
  Power,
  CheckCircle,
  XCircle,
  Server,
  Globe,
  Shield,
  Key,
  Wifi,
  WifiOff,
  Clock,
  HardDrive,
  RefreshCw,
  AlertTriangle,
  X,
  Terminal,
  Code2,
  Loader2,
  Play,
  Square,
  ArrowRightLeft,
  Pencil,
  Rocket,
} from 'lucide-react';
import Card from '../components/Card';
import Button from '../components/Button';
import PageHeader from '../components/PageHeader';
import AppGroupCard from '../components/AppGroupCard';
import CreateContainerModal from '../components/CreateContainerModal';
import DeployModal from '../components/DeployModal';
import useWebSocket from '../hooks/useWebSocket';
import {
  getContainers,
  createContainer,
  updateContainer,
  deleteContainer,
  startContainer,
  stopContainer,
  migrateContainer,
  cancelMigration,
  getReverseProxyConfig,
  getHosts,
  startApplicationService,
  stopApplicationService,
} from '../api/client';

function groupByApp(containers) {
  const groups = new Map();
  containers.forEach(c => {
    const slug = c.slug;
    if (!groups.has(slug)) {
      groups.set(slug, { slug, name: c.name, dev: null, prod: null });
    }
    const g = groups.get(slug);
    if (c.environment === 'production') {
      g.prod = c;
    } else {
      g.dev = c;
    }
    // Use the dev name as group name if available
    if (g.dev) g.name = g.dev.name;
  });
  return Array.from(groups.values());
}

function formatBytes(bytes) {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(0)) + ' ' + sizes[i];
}

// MigrationProgress inline component
function MigrationProgress({ appId, migration, onDismiss }) {
  if (!migration) return null;

  const prevRef = useRef({ bytes: 0, time: Date.now() });
  const speedRef = useRef(0);

  useEffect(() => {
    const now = Date.now();
    const elapsed = (now - prevRef.current.time) / 1000;
    const deltaBytes = migration.bytesTransferred - prevRef.current.bytes;
    if (elapsed > 0.5 && deltaBytes > 0) {
      const instantSpeed = deltaBytes / elapsed;
      speedRef.current = speedRef.current > 0
        ? speedRef.current * 0.6 + instantSpeed * 0.4
        : instantSpeed;
      prevRef.current = { bytes: migration.bytesTransferred, time: now };
    }
  }, [migration.bytesTransferred]);

  const phaseLabels = {
    stopping: 'Arret...',
    exporting: 'Export...',
    transferring: 'Transfert conteneur...',
    transferring_workspace: 'Transfert workspace...',
    importing: 'Import...',
    importing_workspace: 'Import workspace...',
    starting: 'Demarrage...',
    verifying: 'Verification...',
    complete: 'Termine',
    failed: 'Echoue',
  };

  const isActive = migration.phase !== 'complete' && migration.phase !== 'failed';
  const isTransfer = migration.phase === 'transferring' || migration.phase === 'transferring_workspace';
  const speed = speedRef.current;
  const remaining = migration.totalBytes - migration.bytesTransferred;
  const eta = speed > 0 && remaining > 0 ? Math.ceil(remaining / speed) : 0;

  const formatEta = (secs) => {
    if (secs <= 0) return '';
    if (secs < 60) return `${secs}s`;
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return s > 0 ? `${m}m${s}s` : `${m}m`;
  };

  const handleCancel = async () => {
    try {
      await cancelMigration(appId);
    } catch (err) {
      console.error('Cancel migration failed:', err);
    }
  };

  return (
    <div className="p-2 bg-gray-700/50">
      <div className="flex items-center justify-between mb-1">
        <span className={`text-xs ${migration.phase === 'failed' ? 'text-red-400' : migration.phase === 'complete' ? 'text-green-400' : 'text-gray-300'}`}>
          {phaseLabels[migration.phase] || migration.phase}
        </span>
        <div className="flex items-center gap-2">
          <span className="text-xs text-gray-400">{migration.progressPct}%</span>
          {isActive && (
            <button
              onClick={handleCancel}
              className="text-red-400 hover:text-red-300 transition-colors"
              title="Annuler la migration"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          )}
          {!isActive && (
            <button
              onClick={onDismiss}
              className="text-gray-500 hover:text-gray-300 transition-colors"
              title="Fermer"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>
      <div className="w-full bg-gray-600 h-1.5">
        <div
          className={`h-1.5 transition-all duration-500 ${
            migration.phase === 'failed' ? 'bg-red-500' :
            migration.phase === 'complete' ? 'bg-green-500' : 'bg-blue-500'
          }`}
          style={{ width: `${migration.progressPct}%` }}
        />
      </div>
      {migration.totalBytes > 0 && (
        <div className="flex items-center justify-between text-xs text-gray-500 mt-1">
          <span>{formatBytes(migration.bytesTransferred)} / {formatBytes(migration.totalBytes)}</span>
          {isTransfer && speed > 0 && (
            <span>
              {formatBytes(speed)}/s
              {eta > 0 && ` - ${formatEta(eta)}`}
            </span>
          )}
        </div>
      )}
      {migration.error && (
        <div className="text-xs text-red-400 mt-1 select-all cursor-text">{migration.error}</div>
      )}
    </div>
  );
}

function Containers() {
  const [containers, setContainers] = useState([]);
  const [baseDomain, setBaseDomain] = useState('');
  const [hosts, setHosts] = useState([]);
  const [loading, setLoading] = useState(true);
  const [message, setMessage] = useState(null);
  const [saving, setSaving] = useState(false);

  // Modal states
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [createModalDefaults, setCreateModalDefaults] = useState({});
  const [terminalContainer, setTerminalContainer] = useState(null);
  const [migrateModal, setMigrateModal] = useState(null);
  const [selectedHostId, setSelectedHostId] = useState('');
  const [migrating, setMigrating] = useState(false);
  const [migrations, setMigrations] = useState({});
  const [showEditModal, setShowEditModal] = useState(false);
  const [editingContainer, setEditingContainer] = useState(null);
  const [editForm, setEditForm] = useState(null);
  const [deployModal, setDeployModal] = useState(null);

  // Agent metrics state
  const [appMetrics, setAppMetrics] = useState({});

  const fetchData = useCallback(async () => {
    try {
      const [containersRes, configRes, hostsRes] = await Promise.all([
        getContainers(),
        getReverseProxyConfig(),
        getHosts().catch(() => ({ data: { hosts: [] } })),
      ]);
      if (containersRes.data.success !== false) {
        const list = containersRes.data.containers || containersRes.data || [];
        setContainers(list);
        // Pre-populate metrics from initial REST response
        const initialMetrics = {};
        for (const c of list) {
          if (c.metrics) {
            initialMetrics[c.id] = {
              codeServerStatus: c.metrics.code_server_status,
              appStatus: c.metrics.app_status,
              dbStatus: c.metrics.db_status,
              memoryBytes: c.metrics.memory_bytes,
              cpuPercent: c.metrics.cpu_percent,
            };
          }
        }
        if (Object.keys(initialMetrics).length > 0) {
          setAppMetrics(prev => ({ ...initialMetrics, ...prev }));
        }
      }
      if (configRes.data.success) setBaseDomain(configRes.data.config?.baseDomain || '');
      const hostList = hostsRes.data?.hosts || [];
      setHosts(hostList);
    } catch (error) {
      console.error('Error:', error);
      setMessage({ type: 'error', text: 'Erreur de chargement' });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const fetchDataRef = useRef(fetchData);
  fetchDataRef.current = fetchData;

  useWebSocket({
    'agent:status': (data) => {
      const { appId, status, message: stepMsg } = data;
      setContainers(prev => {
        const old = prev.find(c => c.id === appId);
        if (!old) return prev;
        const wasDeploying = old.status === 'deploying' || old._deployMessage;
        const nowReady = status === 'connected' || (status === 'pending' && wasDeploying);
        if (wasDeploying && nowReady) {
          setTimeout(() => fetchDataRef.current(), 500);
        }
        return prev.map(c =>
          c.id === appId
            ? { ...c, status, _deployMessage: status === 'deploying' ? (stepMsg || null) : null }
            : c
        );
      });
    },
    'agent:metrics': (data) => {
      const { appId, codeServerStatus, appStatus, dbStatus, memoryBytes, cpuPercent } = data;
      setAppMetrics(prev => ({
        ...prev,
        [appId]: { codeServerStatus, appStatus, dbStatus, memoryBytes, cpuPercent }
      }));
    },
    'agent:service-command': (data) => {
      const { appId, serviceType, action, success } = data;
      if (success && appId) {
        const statusMap = { started: 'running', stopped: 'stopped', starting: 'starting', stopping: 'stopping' };
        const newStatus = statusMap[action] || action;
        setAppMetrics(prev => {
          const current = prev[appId] || {};
          const updated = { ...current };
          if (serviceType === 'code_server') updated.codeServerStatus = newStatus;
          return { ...prev, [appId]: updated };
        });
      }
    },
    'hosts:status': (data) => {
      const { hostId, status } = data;
      setHosts(prev => prev.map(h =>
        h.id === hostId ? { ...h, status } : h
      ));
    },
    'migration:progress': (data) => {
      setMigrations(prev => ({
        ...prev,
        [data.appId]: {
          phase: data.phase,
          progressPct: data.progressPct,
          bytesTransferred: data.bytesTransferred,
          totalBytes: data.totalBytes,
          error: data.error,
        }
      }));
      if (data.phase === 'complete') {
        setTimeout(() => fetchDataRef.current(), 1000);
        setTimeout(() => {
          setMigrations(prev => {
            const next = { ...prev };
            delete next[data.appId];
            return next;
          });
        }, 5000);
      }
    },
  });

  // Auto-dismiss messages
  useEffect(() => {
    if (message) {
      const timer = setTimeout(() => setMessage(null), 4000);
      return () => clearTimeout(timer);
    }
  }, [message]);

  async function handleCreate(payload) {
    if (!payload.name || !payload.slug) {
      setMessage({ type: 'error', text: 'Nom et slug requis' });
      return;
    }
    setSaving(true);
    try {
      const res = await createContainer(payload);
      if (res.data.success) {
        setShowCreateModal(false);
        setCreateModalDefaults({});
        setMessage({ type: 'success', text: 'Conteneur cree' });
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error || 'Erreur' });
      }
    } catch (error) {
      setMessage({ type: 'error', text: error.response?.data?.error || 'Erreur' });
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(id, name) {
    if (!confirm(`Supprimer "${name}" ?\nCeci detruira le conteneur nspawn, les enregistrements DNS et les certificats.`)) return;
    try {
      const res = await deleteContainer(id);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Conteneur supprime' });
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch {
      setMessage({ type: 'error', text: 'Erreur' });
    }
  }

  async function handleStart(id) {
    try {
      const res = await startContainer(id);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Conteneur demarre' });
        fetchData();
        setTimeout(() => fetchData(), 5000);
        setTimeout(() => fetchData(), 15000);
      } else {
        setMessage({ type: 'error', text: res.data.error || 'Erreur' });
      }
    } catch {
      setMessage({ type: 'error', text: 'Erreur' });
    }
  }

  async function handleStop(id) {
    try {
      const res = await stopContainer(id);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Conteneur arrete' });
        fetchData();
        setTimeout(() => fetchData(), 3000);
      } else {
        setMessage({ type: 'error', text: res.data.error || 'Erreur' });
      }
    } catch {
      setMessage({ type: 'error', text: 'Erreur' });
    }
  }

  function openEditModal(container) {
    setEditingContainer(container);
    setEditForm({
      name: container.name,
      frontend: {
        auth_required: container.frontend?.auth_required || false,
        allowed_groups: container.frontend?.allowed_groups || [],
        local_only: container.frontend?.local_only || false,
      },
      code_server_enabled: container.code_server_enabled !== false,
    });
    setShowEditModal(true);
  }

  async function handleEdit() {
    if (!editForm || !editingContainer) return;
    setSaving(true);
    try {
      const payload = {
        name: editForm.name,
        frontend: {
          target_port: 3000,
          auth_required: editForm.frontend.auth_required,
          allowed_groups: editForm.frontend.allowed_groups,
          local_only: editForm.frontend.local_only,
        },
        code_server_enabled: editForm.code_server_enabled,
      };
      const res = await updateContainer(editingContainer.id, payload);
      if (res.data.success) {
        setShowEditModal(false);
        setEditingContainer(null);
        setEditForm(null);
        setMessage({ type: 'success', text: 'Conteneur modifie' });
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error || 'Erreur' });
      }
    } catch (error) {
      setMessage({ type: 'error', text: error.response?.data?.error || 'Erreur' });
    } finally {
      setSaving(false);
    }
  }

  const openMigrateModal = async (container) => {
    try {
      const res = await getHosts();
      const hostList = res.data.hosts || res.data || [];
      setHosts(hostList);
      setMigrateModal(container);
      setSelectedHostId('');
    } catch (err) {
      console.error('Failed to fetch hosts:', err);
    }
  };

  const handleMigrate = async () => {
    if (!migrateModal || !selectedHostId) return;
    const targetHost = hosts.find(h => h.id === selectedHostId);
    const targetName = selectedHostId === 'local' ? 'HomeRoute (local)' : (targetHost?.name || selectedHostId);
    if (!confirm(`Migrer ${migrateModal.name} vers ${targetName} ?\n\nLe conteneur sera arrete pendant la migration.`)) return;
    setMigrating(true);
    try {
      await migrateContainer(migrateModal.id, selectedHostId);
      setMigrateModal(null);
    } catch (err) {
      console.error('Migration failed:', err);
      alert(err.response?.data?.error || 'Migration failed');
    } finally {
      setMigrating(false);
    }
  };

  function handleCreatePaired(slug, name, environment, linkedAppId) {
    setCreateModalDefaults({ slug, name, environment, linkedAppId });
    setShowCreateModal(true);
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-400"></div>
      </div>
    );
  }

  const groups = groupByApp(containers);
  const devRunning = containers.filter(c => (c.environment || 'development') !== 'production' && (c.agent_status || c.status) === 'connected').length;
  const prodRunning = containers.filter(c => c.environment === 'production' && (c.agent_status || c.status) === 'connected').length;
  const deployingCount = containers.filter(c => (c.agent_status || c.status) === 'deploying' || c.status === 'deploying').length;

  return (
    <div>
      <PageHeader title="Containers" icon={Container}>
        <Button onClick={fetchData} variant="secondary">
          <RefreshCw className="w-4 h-4" />
          Rafraichir
        </Button>
        <Button onClick={() => { setCreateModalDefaults({}); setShowCreateModal(true); }}>
          <Plus className="w-4 h-4" />
          Nouveau conteneur
        </Button>
      </PageHeader>

      {/* Message */}
      {message && (
        <div className={`p-4 flex items-center gap-2 ${
          message.type === 'success' ? 'bg-green-900/50 text-green-400' : 'bg-red-900/50 text-red-400'
        }`}>
          {message.type === 'success' ? <CheckCircle className="w-5 h-5" /> : <XCircle className="w-5 h-5" />}
          {message.text}
        </div>
      )}

      {/* Stats */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-px">
        <Card title="Total" icon={Container}>
          <div className="text-2xl font-bold">{containers.length}</div>
        </Card>
        <Card title="Dev Running" icon={Wifi}>
          <div className="text-2xl font-bold text-blue-400">{devRunning}</div>
        </Card>
        <Card title="Prod Running" icon={Wifi}>
          <div className="text-2xl font-bold text-purple-400">{prodRunning}</div>
        </Card>
        <Card title="Deploying" icon={Loader2}>
          <div className="text-2xl font-bold text-blue-400">{deployingCount}</div>
        </Card>
      </div>

      {/* App Groups */}
      <div>
        {groups.length === 0 ? (
          <Card>
            <div className="text-center py-8 text-gray-500">
              <Container className="w-12 h-12 mx-auto mb-2 opacity-50" />
              <p>Aucun conteneur</p>
              <p className="text-xs mt-2">Creez un conteneur nspawn pour deployer une application</p>
            </div>
          </Card>
        ) : (
          groups.map(group => (
            <AppGroupCard
              key={group.slug}
              group={group}
              baseDomain={baseDomain}
              appMetrics={appMetrics}
              migrations={migrations}
              hosts={hosts}
              onStart={handleStart}
              onStop={handleStop}
              onTerminal={setTerminalContainer}
              onEdit={openEditModal}
              onMigrate={openMigrateModal}
              onDelete={handleDelete}
              onMigrationDismiss={(id) => setMigrations(prev => {
                const next = { ...prev };
                delete next[id];
                return next;
              })}
              onDeploy={(dev, prod) => setDeployModal({ dev, prod })}
              onCreatePaired={handleCreatePaired}
              MigrationProgress={MigrationProgress}
            />
          ))
        )}
      </div>

      {/* Create Modal */}
      {showCreateModal && (
        <CreateContainerModal
          baseDomain={baseDomain}
          hosts={hosts}
          containers={containers}
          onClose={() => { setShowCreateModal(false); setCreateModalDefaults({}); }}
          onCreate={handleCreate}
          saving={saving}
          initialEnvironment={createModalDefaults.environment}
          initialSlug={createModalDefaults.slug}
          initialName={createModalDefaults.name}
          initialLinkedAppId={createModalDefaults.linkedAppId}
        />
      )}

      {/* Deploy Modal */}
      {deployModal && (
        <DeployModal
          devContainer={deployModal.dev}
          prodContainer={deployModal.prod}
          baseDomain={baseDomain}
          onClose={() => setDeployModal(null)}
          onDeployStarted={() => {
            fetchData();
            setDeployModal(null);
          }}
        />
      )}

      {/* Edit Modal */}
      {showEditModal && editingContainer && editForm && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
          <div className="bg-gray-800 p-6 w-full max-w-2xl border border-gray-700 max-h-[90vh] overflow-y-auto">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-bold">Modifier {editingContainer.name}</h2>
              <div className="flex items-center gap-2">
                <span className={`text-xs px-1.5 py-0.5 font-medium ${
                  editingContainer.environment === 'production'
                    ? 'bg-purple-100 text-purple-800'
                    : 'bg-blue-100 text-blue-800'
                }`}>
                  {editingContainer.environment === 'production' ? 'PROD' : 'DEV'}
                </span>
                <span className="text-xs text-gray-500 bg-gray-900/50 px-2 py-1 font-mono">{editingContainer.slug}</span>
              </div>
            </div>
            <div className="space-y-4">
              <div>
                <label className="block text-sm text-gray-400 mb-1">Nom d&apos;affichage</label>
                <input
                  type="text"
                  value={editForm.name}
                  onChange={e => setEditForm({ ...editForm, name: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 text-sm"
                />
              </div>

              {/* Frontend */}
              <div>
                <div className="text-xs text-blue-400 mb-2 font-mono flex items-center gap-1">
                  <Globe className="w-3 h-3" />
                  {editingContainer.environment === 'production'
                    ? `${editingContainer.slug}.${baseDomain}`
                    : `dev.${editingContainer.slug}.${baseDomain}`
                  }
                </div>
                <div className="flex items-center gap-4">
                  <label className="flex items-center gap-1.5 text-xs cursor-pointer">
                    <input
                      type="checkbox"
                      checked={editForm.frontend.auth_required}
                      onChange={e => setEditForm({ ...editForm, frontend: { ...editForm.frontend, auth_required: e.target.checked } })}
                      className="rounded"
                    />
                    <Key className="w-3 h-3 text-purple-400" /> Auth
                  </label>
                  <label className="flex items-center gap-1.5 text-xs cursor-pointer">
                    <input
                      type="checkbox"
                      checked={editForm.frontend.local_only}
                      onChange={e => setEditForm({ ...editForm, frontend: { ...editForm.frontend, local_only: e.target.checked } })}
                      className="rounded"
                    />
                    <Shield className="w-3 h-3 text-yellow-400" /> Local
                  </label>
                </div>
              </div>

              {/* code-server (dev only) */}
              {editingContainer.environment !== 'production' && (
                <label className="flex items-center gap-2 text-sm cursor-pointer">
                  <input
                    type="checkbox"
                    checked={editForm.code_server_enabled}
                    onChange={e => setEditForm({ ...editForm, code_server_enabled: e.target.checked })}
                    className="rounded"
                  />
                  <Code2 className="w-4 h-4 text-cyan-400" />
                  code-server IDE
                  {baseDomain && editForm.code_server_enabled && (
                    <span className="text-xs text-gray-500 font-mono ml-2">code.{editingContainer.slug}.{baseDomain}</span>
                  )}
                </label>
              )}
            </div>
            <div className="flex justify-end gap-2 mt-6">
              <Button variant="secondary" onClick={() => { setShowEditModal(false); setEditingContainer(null); }}>Annuler</Button>
              <Button onClick={handleEdit} loading={saving}>Sauvegarder</Button>
            </div>
          </div>
        </div>
      )}

      {/* Migrate Modal */}
      {migrateModal && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-gray-800 p-6 w-full max-w-md border border-gray-700">
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-lg font-semibold">Migrer {migrateModal.name}</h3>
              <button onClick={() => setMigrateModal(null)} className="p-1 text-gray-400 hover:text-white">
                <X className="w-5 h-5" />
              </button>
            </div>
            <p className="text-sm text-gray-400 mb-4">
              Selectionnez l&apos;hote de destination pour migrer ce conteneur.
            </p>
            <select
              value={selectedHostId}
              onChange={(e) => setSelectedHostId(e.target.value)}
              className="w-full px-3 py-2 bg-gray-700 border border-gray-600 text-white mb-4"
            >
              <option value="">Choisir un hote...</option>
              {hosts
                .filter(h => h.id !== migrateModal.host_id && h.name !== 'HomeRoute')
                .map(h => (
                  <option key={h.id} value={h.id}>
                    {h.name} ({h.host}) — {h.status}
                  </option>
                ))
              }
              {migrateModal.host_id !== 'local' && (
                <option value="local">HomeRoute (local)</option>
              )}
            </select>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setMigrateModal(null)}
                className="px-4 py-2 text-gray-300 hover:text-white transition-colors"
              >
                Annuler
              </button>
              <button
                onClick={handleMigrate}
                disabled={!selectedHostId || migrating}
                className="px-4 py-2 bg-blue-600 hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed text-white transition-colors flex items-center gap-2"
              >
                {migrating && <Loader2 className="w-4 h-4 animate-spin" />}
                Migrer
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Terminal Modal */}
      {terminalContainer && (
        <TerminalModal container={terminalContainer} onClose={() => setTerminalContainer(null)} />
      )}
    </div>
  );
}

function TerminalModal({ container, onClose }) {
  const termRef = useRef(null);
  const termInstance = useRef(null);
  const wsRef = useRef(null);
  const fitAddonRef = useRef(null);

  useEffect(() => {
    let cancelled = false;

    async function init() {
      const { Terminal: XTerm } = await import('@xterm/xterm');
      const { FitAddon } = await import('@xterm/addon-fit');
      await import('@xterm/xterm/css/xterm.css');

      if (cancelled || !termRef.current) return;

      const fitAddon = new FitAddon();
      fitAddonRef.current = fitAddon;

      const term = new XTerm({
        cursorBlink: true,
        fontSize: 14,
        fontFamily: 'Menlo, Monaco, "Courier New", monospace',
        theme: {
          background: '#111827',
          foreground: '#e5e7eb',
          cursor: '#10b981',
          selectionBackground: '#374151',
        },
      });

      term.loadAddon(fitAddon);
      term.open(termRef.current);
      fitAddon.fit();
      termInstance.current = term;

      const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      const ws = new WebSocket(`${proto}//${window.location.host}/api/containers/${container.id}/terminal`);
      ws.binaryType = 'arraybuffer';
      wsRef.current = ws;

      ws.onopen = () => {
        term.write('\r\n\x1b[32mConnexion au conteneur ' + container.container_name + '...\x1b[0m\r\n\r\n');
      };

      ws.onmessage = (event) => {
        if (event.data instanceof ArrayBuffer) {
          term.write(new Uint8Array(event.data));
        } else {
          term.write(event.data);
        }
      };

      ws.onclose = () => {
        term.write('\r\n\x1b[31mConnexion fermee.\x1b[0m\r\n');
      };

      ws.onerror = () => {
        term.write('\r\n\x1b[31mErreur de connexion.\x1b[0m\r\n');
      };

      term.onData((data) => {
        if (ws.readyState === WebSocket.OPEN) {
          ws.send(data);
        }
      });

      const handleResize = () => {
        fitAddon.fit();
      };
      window.addEventListener('resize', handleResize);

      return () => {
        window.removeEventListener('resize', handleResize);
      };
    }

    init();

    return () => {
      cancelled = true;
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
      if (termInstance.current) {
        termInstance.current.dispose();
        termInstance.current = null;
      }
    };
  }, [container]);

  return (
    <div className="fixed inset-0 bg-black/80 flex flex-col z-50">
      <div className="flex items-center justify-between px-4 py-2 bg-gray-900 border-b border-gray-700">
        <div className="flex items-center gap-2 text-sm">
          <Terminal className="w-4 h-4 text-emerald-400" />
          <span className="font-medium">{container.name}</span>
          <span className="text-gray-500 font-mono">({container.container_name})</span>
        </div>
        <button
          onClick={onClose}
          className="text-gray-400 hover:text-white p-1 transition-colors"
        >
          <X className="w-5 h-5" />
        </button>
      </div>
      <div ref={termRef} className="flex-1 p-2" style={{ backgroundColor: '#111827' }} />
    </div>
  );
}

export default Containers;
