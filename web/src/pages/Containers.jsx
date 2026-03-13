import { useState, useEffect, useRef, useCallback } from 'react';
import {
  Container,
  Plus,
  CheckCircle,
  XCircle,
  RefreshCw,
  AlertTriangle,
  X,
  Terminal,
  Loader2,
} from 'lucide-react';
import Button from '../components/Button';
import PageHeader from '../components/PageHeader';
import ApplicationCard from '../components/ApplicationCard';
import CreateContainerModal from '../components/CreateContainerModal';
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
  renameContainer,
  getRenameStatus,
} from '../api/client';

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
      <div className="w-full bg-gray-600 h-1.5 rounded-full overflow-hidden">
        <div
          className={`h-1.5 rounded-full transition-all duration-500 ${
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
  const [terminalContainer, setTerminalContainer] = useState(null);
  const [migrateModal, setMigrateModal] = useState(null);
  const [selectedHostId, setSelectedHostId] = useState('');
  const [migrating, setMigrating] = useState(false);
  const [migrations, setMigrations] = useState({});
  const [editingApp, setEditingApp] = useState(null);
  const [appEditForm, setAppEditForm] = useState({ name: '', slug: '' });
  const [renameProgress, setRenameProgress] = useState(null);

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
      const { appId, appStatus, dbStatus, memoryBytes, cpuPercent } = data;
      setAppMetrics(prev => ({
        ...prev,
        [appId]: { appStatus, dbStatus, memoryBytes, cpuPercent }
      }));
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

  async function handleDelete(container) {
    if (!confirm(`Supprimer "${container.name || container.slug}" ?\nCeci detruira le conteneur nspawn, les enregistrements DNS et les certificats.`)) return;
    try {
      const res = await deleteContainer(container.id);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Application supprimee' });
      } else {
        setMessage({ type: 'error', text: res.data.error || 'Erreur' });
      }
      fetchData();
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
    setEditingApp(container);
    setAppEditForm({ name: container.name || container.slug, slug: container.slug });
    setRenameProgress(null);
  }

  async function handleAppEdit() {
    if (!editingApp) return;
    const slugChanged = appEditForm.slug !== editingApp.slug;

    setSaving(true);
    try {
      if (slugChanged) {
        const res = await renameContainer(editingApp.id, {
          new_slug: appEditForm.slug,
          new_name: appEditForm.name,
        });
        if (res.data.success) {
          setRenameProgress({ phase: 'started' });
          const pollInterval = setInterval(async () => {
            try {
              const statusRes = await getRenameStatus(editingApp.id);
              const status = statusRes.data;
              setRenameProgress(status);
              if (status.phase === 'complete' || status.phase === 'failed') {
                clearInterval(pollInterval);
                if (status.phase === 'complete') {
                  setEditingApp(null);
                  setMessage({ type: 'success', text: 'Application renommee' });
                  fetchData();
                } else {
                  setMessage({ type: 'error', text: status.error || 'Echec du renommage' });
                }
                setSaving(false);
              }
            } catch {
              clearInterval(pollInterval);
              setSaving(false);
            }
          }, 2000);
        } else {
          setMessage({ type: 'error', text: res.data.error || 'Erreur' });
          setSaving(false);
        }
      } else {
        const res = await updateContainer(editingApp.id, { name: appEditForm.name });
        if (res.data.success) {
          setEditingApp(null);
          setMessage({ type: 'success', text: 'Application modifiee' });
          fetchData();
        } else {
          setMessage({ type: 'error', text: res.data.error || 'Erreur' });
        }
        setSaving(false);
      }
    } catch (error) {
      setMessage({ type: 'error', text: error.response?.data?.error || 'Erreur' });
      setSaving(false);
    }
  }

  async function handleToggleSecurity(containerId, field, newValue) {
    try {
      const res = await updateContainer(containerId, {
        frontend: { target_port: 3000, [field]: newValue },
      });
      if (res.data.success) {
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error || 'Erreur' });
      }
    } catch (error) {
      setMessage({ type: 'error', text: error.response?.data?.error || 'Erreur' });
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

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-400"></div>
      </div>
    );
  }

  const prodContainers = containers.filter(c => c.environment === 'production');
  const connectedCount = prodContainers.filter(c => (c.agent_status || c.status) === 'connected').length;

  return (
    <div>
      <PageHeader title="Applications" icon={Container}>
        <span className="text-sm text-gray-400 hidden sm:inline">
          {prodContainers.length} application{prodContainers.length !== 1 ? 's' : ''} · {connectedCount} connectee{connectedCount !== 1 ? 's' : ''}
        </span>
        <Button onClick={fetchData} variant="secondary">
          <RefreshCw className="w-4 h-4" />
          Rafraichir
        </Button>
        <Button onClick={() => setShowCreateModal(true)}>
          <Plus className="w-4 h-4" />
          Nouvelle application
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

      {/* Card Grid */}
      <div className="p-4">
        {prodContainers.length === 0 ? (
          <div className="text-center py-12 text-gray-500">
            <Container className="w-12 h-12 mx-auto mb-2 opacity-50" />
            <p>Aucune application</p>
            <p className="text-xs mt-2">Creez une application pour commencer</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
            {prodContainers.map(container => (
              <ApplicationCard
                key={container.id}
                container={container}
                baseDomain={baseDomain}
                metrics={appMetrics[container.id]}
                migration={migrations[container.id]}
                hosts={hosts}
                onStart={handleStart}
                onStop={handleStop}
                onTerminal={setTerminalContainer}
                onEdit={openEditModal}
                onDelete={handleDelete}
                onToggleSecurity={handleToggleSecurity}
                onMigrate={openMigrateModal}
                onMigrationDismiss={(id) => setMigrations(prev => {
                  const next = { ...prev };
                  delete next[id];
                  return next;
                })}
                MigrationProgress={MigrationProgress}
              />
            ))}
          </div>
        )}
      </div>

      {/* Create Modal */}
      {showCreateModal && (
        <CreateContainerModal
          baseDomain={baseDomain}
          hosts={hosts}
          containers={containers}
          onClose={() => setShowCreateModal(false)}
          onCreate={handleCreate}
          saving={saving}
        />
      )}

      {/* App Edit Modal */}
      {editingApp && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
          <div className="bg-gray-800 p-6 w-full max-w-md border border-gray-700 rounded-lg">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-bold">Modifier {editingApp.name || editingApp.slug}</h2>
              <button onClick={() => setEditingApp(null)} className="p-1 text-gray-400 hover:text-white">
                <X className="w-5 h-5" />
              </button>
            </div>
            <div className="space-y-4">
              <div>
                <label className="block text-sm text-gray-400 mb-1">Nom d&apos;affichage</label>
                <input
                  type="text"
                  value={appEditForm.name}
                  onChange={e => setAppEditForm({ ...appEditForm, name: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm"
                />
              </div>
              <div>
                <label className="block text-sm text-gray-400 mb-1">Slug</label>
                <input
                  type="text"
                  value={appEditForm.slug}
                  onChange={e => setAppEditForm({ ...appEditForm, slug: e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, '') })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm font-mono"
                />
                {(appEditForm.slug.length < 3 || appEditForm.slug.length > 32) && appEditForm.slug.length > 0 && (
                  <p className="text-xs text-red-400 mt-1">Le slug doit contenir entre 3 et 32 caracteres</p>
                )}
              </div>
              {appEditForm.slug !== editingApp.slug && appEditForm.slug.length >= 3 && (
                <div className="p-3 bg-yellow-900/30 border border-yellow-700/50 rounded text-yellow-400 text-xs">
                  <AlertTriangle className="w-3.5 h-3.5 inline mr-1" />
                  Le renommage entrainera un bref downtime (~2min)
                </div>
              )}
              {renameProgress && renameProgress.phase && renameProgress.phase !== 'complete' && renameProgress.phase !== 'failed' && (
                <div className="p-3 bg-blue-900/30 border border-blue-700/50 rounded text-blue-400 text-xs flex items-center gap-2">
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  {renameProgress.phase}...
                </div>
              )}
            </div>
            <div className="flex justify-end gap-2 mt-6">
              <Button variant="secondary" onClick={() => setEditingApp(null)} disabled={saving}>Annuler</Button>
              <Button
                onClick={handleAppEdit}
                loading={saving}
                disabled={saving || appEditForm.slug.length < 3 || appEditForm.slug.length > 32}
              >
                Sauvegarder
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Migrate Modal */}
      {migrateModal && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
          <div className="bg-gray-800 p-6 w-full max-w-md border border-gray-700 rounded-lg">
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
              className="w-full px-3 py-2 bg-gray-700 border border-gray-600 rounded text-white mb-4"
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
                className="px-4 py-2 bg-blue-600 hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed text-white rounded transition-colors flex items-center gap-2"
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
