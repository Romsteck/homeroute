import { useState, useEffect, useCallback } from 'react';
import {
  Layers,
  Plus,
  Play,
  Square,
  ExternalLink,
  CheckCircle,
  XCircle,
  Wifi,
  WifiOff,
  Trash2,
  X,
  Loader2,
  RefreshCw,
  Check,
} from 'lucide-react';
import Button from '../components/Button';
import PageHeader from '../components/PageHeader';
import StatusBadge from '../components/StatusBadge';
import {
  getEnvironments,
  createEnvironment,
  startEnvironment,
  stopEnvironment,
  deleteEnvironment,
} from '../api/client';
import useWebSocket from '../hooks/useWebSocket';

function Environments() {
  const [environments, setEnvironments] = useState([]);
  const [loading, setLoading] = useState(true);
  const [message, setMessage] = useState(null);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [creating, setCreating] = useState(false);
  const [formData, setFormData] = useState({ name: '', slug: '', env_type: 'dev' });
  const [formError, setFormError] = useState('');

  const fetchData = useCallback(async () => {
    try {
      const res = await getEnvironments();
      const envs = res.data.environments || [];
      setEnvironments(Array.isArray(envs) ? envs : []);
    } catch (error) {
      console.error('Error loading environments:', error);
      setMessage({ type: 'error', text: 'Erreur de chargement des environnements' });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  // Listen for real-time environment status updates
  useWebSocket({
    'environment:status': (data) => {
      setEnvironments(prev =>
        prev.map(env =>
          env.slug === data.slug ? { ...env, status: data.status } : env
        )
      );
    },
    'agent:status': () => {
      // Refetch when agent status changes since it affects environment state
      fetchData();
    },
  });

  // Auto-dismiss messages
  useEffect(() => {
    if (message) {
      const timer = setTimeout(() => setMessage(null), 4000);
      return () => clearTimeout(timer);
    }
  }, [message]);

  // ── Actions ─────────────────────────────────

  async function handleCreate(e) {
    e.preventDefault();
    if (!formData.name || !formData.slug) {
      setFormError('Nom et slug requis');
      return;
    }
    setCreating(true);
    setFormError('');
    try {
      const res = await createEnvironment(formData);
      if (res.data.success) {
        setShowCreateModal(false);
        setFormData({ name: '', slug: '', env_type: 'dev' });
        setMessage({ type: 'success', text: 'Environnement cree' });
        fetchData();
      } else {
        setFormError(res.data.error || 'Erreur');
      }
    } catch (error) {
      setFormError(error.response?.data?.error || 'Erreur de creation');
    } finally {
      setCreating(false);
    }
  }

  async function handleStart(slug) {
    try {
      const res = await startEnvironment(slug);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Environnement demarre' });
        fetchData();
        setTimeout(() => fetchData(), 5000);
      } else {
        setMessage({ type: 'error', text: res.data.error || 'Erreur' });
      }
    } catch {
      setMessage({ type: 'error', text: 'Erreur au demarrage' });
    }
  }

  async function handleStop(slug) {
    try {
      const res = await stopEnvironment(slug);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Environnement arrete' });
        fetchData();
        setTimeout(() => fetchData(), 3000);
      } else {
        setMessage({ type: 'error', text: res.data.error || 'Erreur' });
      }
    } catch {
      setMessage({ type: 'error', text: "Erreur a l'arret" });
    }
  }

  async function handleDelete(env) {
    if (!confirm(`Supprimer l'environnement "${env.name || env.slug}" ?\nCeci detruira les conteneurs associes.`)) return;
    try {
      const res = await deleteEnvironment(env.slug);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Environnement supprime' });
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error || 'Erreur' });
      }
    } catch {
      setMessage({ type: 'error', text: 'Erreur de suppression' });
    }
  }

  // ── Helpers ─────────────────────────────────

  function getEnvStatusBadge(status) {
    switch (status) {
      case 'running': return 'up';
      case 'stopped': return 'down';
      case 'starting':
      case 'stopping': return 'active';
      default: return 'unknown';
    }
  }

  function getEnvStatusLabel(status) {
    switch (status) {
      case 'running': return 'En ligne';
      case 'stopped': return 'Arrete';
      case 'starting': return 'Demarrage...';
      case 'stopping': return 'Arret...';
      default: return status || 'Inconnu';
    }
  }

  function getTypeBadgeClass(envType) {
    switch (envType) {
      case 'prod':
      case 'production':
        return 'bg-green-500/20 text-green-400 border-green-500/30';
      case 'dev':
      case 'development':
        return 'bg-blue-500/20 text-blue-400 border-blue-500/30';
      case 'staging':
        return 'bg-yellow-500/20 text-yellow-400 border-yellow-500/30';
      default:
        return 'bg-gray-500/20 text-gray-400 border-gray-500/30';
    }
  }

  function getTypeLabel(envType) {
    switch (envType) {
      case 'prod':
      case 'production': return 'PROD';
      case 'dev':
      case 'development': return 'DEV';
      case 'staging': return 'STAGING';
      default: return (envType || 'DEV').toUpperCase();
    }
  }

  function getAgentIcon(env) {
    const connected = env.agent_connected || env.agent_status === 'connected';
    if (connected) {
      return <Wifi className="w-3.5 h-3.5 text-green-400" title="Agent connecte" />;
    }
    return <WifiOff className="w-3.5 h-3.5 text-gray-600" title="Agent deconnecte" />;
  }

  // ── Render ──────────────────────────────────

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-400"></div>
      </div>
    );
  }

  const runningCount = environments.filter(e => e.status === 'running').length;

  return (
    <div>
      <PageHeader title="Environnements" icon={Layers}>
        <span className="text-sm text-gray-400 hidden sm:inline">
          {environments.length} environnement{environments.length !== 1 ? 's' : ''} · {runningCount} actif{runningCount !== 1 ? 's' : ''}
        </span>
        <Button onClick={() => setShowCreateModal(true)}>
          <Plus className="w-4 h-4" />
          Nouvel environnement
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

      {/* Environments list */}
      <div className="p-4">
        {environments.length === 0 ? (
          <div className="text-center py-12 text-gray-500">
            <Layers className="w-12 h-12 mx-auto mb-2 opacity-50" />
            <p>Aucun environnement</p>
            <p className="text-xs mt-2">Creez un environnement pour commencer</p>
          </div>
        ) : (
          <>
            {/* Desktop: Table view */}
            <div className="hidden md:block overflow-x-auto">
              <table className="w-full text-left">
                <thead>
                  <tr className="border-b border-gray-700 text-xs text-gray-500 uppercase tracking-wider">
                    <th className="px-3 py-2 font-medium">Nom</th>
                    <th className="px-3 py-2 font-medium">Type</th>
                    <th className="px-3 py-2 font-medium">Statut</th>
                    <th className="px-3 py-2 font-medium">Agent</th>
                    <th className="px-3 py-2 font-medium hidden lg:table-cell">IP</th>
                    <th className="px-3 py-2 font-medium hidden lg:table-cell">Apps</th>
                    <th className="px-3 py-2 font-medium hidden xl:table-cell">Hote</th>
                    <th className="px-3 py-2 font-medium">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {environments.map(env => (
                    <tr key={env.slug || env.id} className="border-b border-gray-700/50 hover:bg-gray-800/50">
                      {/* Name */}
                      <td className="px-3 py-2.5 text-sm font-medium text-white">
                        <div className="flex items-center gap-2">
                          <Layers className="w-4 h-4 text-blue-400 flex-shrink-0" />
                          {env.name || env.slug}
                        </div>
                      </td>
                      {/* Type */}
                      <td className="px-3 py-2.5">
                        <span className={`px-1.5 py-0.5 text-[10px] font-medium border ${getTypeBadgeClass(env.env_type || env.type)}`}>
                          {getTypeLabel(env.env_type || env.type)}
                        </span>
                      </td>
                      {/* Status */}
                      <td className="px-3 py-2.5">
                        <StatusBadge status={getEnvStatusBadge(env.status)}>
                          {getEnvStatusLabel(env.status)}
                        </StatusBadge>
                      </td>
                      {/* Agent */}
                      <td className="px-3 py-2.5">
                        {getAgentIcon(env)}
                      </td>
                      {/* IP */}
                      <td className="px-3 py-2.5 hidden lg:table-cell">
                        <span className="text-sm font-mono text-gray-400">
                          {env.ipv4_address || env.ip || env.container_ip || '--'}
                        </span>
                      </td>
                      {/* App count */}
                      <td className="px-3 py-2.5 hidden lg:table-cell">
                        <span className="text-sm text-gray-400">
                          {env.app_count ?? (env.apps ? env.apps.length : 0)}
                        </span>
                      </td>
                      {/* Host */}
                      <td className="px-3 py-2.5 hidden xl:table-cell">
                        <span className="text-sm text-gray-400">
                          {env.host_name || env.host_id || 'local'}
                        </span>
                      </td>
                      {/* Actions */}
                      <td className="px-3 py-2.5">
                        <div className="flex items-center gap-1">
                          {env.status === 'running' ? (
                            <button
                              onClick={() => handleStop(env.slug)}
                              className="p-1.5 text-red-400 hover:bg-red-600/20"
                              title="Arreter"
                            >
                              <Square className="w-3.5 h-3.5" />
                            </button>
                          ) : (
                            <button
                              onClick={() => handleStart(env.slug)}
                              className="p-1.5 text-green-400 hover:bg-green-600/20"
                              title="Demarrer"
                            >
                              <Play className="w-3.5 h-3.5" />
                            </button>
                          )}
                          {env.maker_url && (
                            <a
                              href={env.maker_url}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="p-1.5 text-blue-400 hover:bg-blue-600/20"
                              title="Ouvrir Maker Portal"
                            >
                              <ExternalLink className="w-3.5 h-3.5" />
                            </a>
                          )}
                          {env.studio_url && (
                            <a
                              href={env.studio_url}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="p-1.5 text-purple-400 hover:bg-purple-600/20"
                              title="Ouvrir Studio"
                            >
                              <ExternalLink className="w-3.5 h-3.5" />
                            </a>
                          )}
                          <button
                            onClick={() => handleDelete(env)}
                            className="p-1.5 text-red-400 hover:bg-red-600/20"
                            title="Supprimer"
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </button>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            {/* Mobile: Card view */}
            <div className="md:hidden grid grid-cols-1 gap-3">
              {environments.map(env => (
                <div key={env.slug || env.id} className="bg-gray-800 border border-gray-700 rounded-lg p-4">
                  <div className="flex items-center justify-between mb-3">
                    <div className="flex items-center gap-2">
                      <Layers className="w-4 h-4 text-blue-400" />
                      <span className="font-medium text-white">{env.name || env.slug}</span>
                      <span className={`px-1.5 py-0.5 text-[10px] font-medium border ${getTypeBadgeClass(env.env_type || env.type)}`}>
                        {getTypeLabel(env.env_type || env.type)}
                      </span>
                    </div>
                    {getAgentIcon(env)}
                  </div>

                  <div className="grid grid-cols-2 gap-2 text-sm mb-3">
                    <div>
                      <span className="text-gray-500 text-xs">Statut</span>
                      <div className="mt-0.5">
                        <StatusBadge status={getEnvStatusBadge(env.status)}>
                          {getEnvStatusLabel(env.status)}
                        </StatusBadge>
                      </div>
                    </div>
                    <div>
                      <span className="text-gray-500 text-xs">IP</span>
                      <div className="mt-0.5 font-mono text-gray-300 text-sm">
                        {env.ipv4_address || env.ip || env.container_ip || '--'}
                      </div>
                    </div>
                    <div>
                      <span className="text-gray-500 text-xs">Apps</span>
                      <div className="mt-0.5 text-gray-300">
                        {env.app_count ?? (env.apps ? env.apps.length : 0)}
                      </div>
                    </div>
                    <div>
                      <span className="text-gray-500 text-xs">Hote</span>
                      <div className="mt-0.5 text-gray-300">
                        {env.host_name || env.host_id || 'local'}
                      </div>
                    </div>
                  </div>

                  <div className="flex items-center gap-1 border-t border-gray-700 pt-3">
                    {env.status === 'running' ? (
                      <button
                        onClick={() => handleStop(env.slug)}
                        className="p-1.5 text-red-400 hover:bg-red-600/20 rounded"
                        title="Arreter"
                      >
                        <Square className="w-4 h-4" />
                      </button>
                    ) : (
                      <button
                        onClick={() => handleStart(env.slug)}
                        className="p-1.5 text-green-400 hover:bg-green-600/20 rounded"
                        title="Demarrer"
                      >
                        <Play className="w-4 h-4" />
                      </button>
                    )}
                    {env.maker_url && (
                      <a
                        href={env.maker_url}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="p-1.5 text-blue-400 hover:bg-blue-600/20 rounded text-xs flex items-center gap-1"
                      >
                        <ExternalLink className="w-4 h-4" />
                        Maker
                      </a>
                    )}
                    {env.studio_url && (
                      <a
                        href={env.studio_url}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="p-1.5 text-purple-400 hover:bg-purple-600/20 rounded text-xs flex items-center gap-1"
                      >
                        <ExternalLink className="w-4 h-4" />
                        Studio
                      </a>
                    )}
                    <div className="flex-1" />
                    <button
                      onClick={() => handleDelete(env)}
                      className="p-1.5 text-red-400 hover:bg-red-600/20 rounded"
                      title="Supprimer"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </>
        )}
      </div>

      {/* Create Environment Modal */}
      {showCreateModal && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50 p-4">
          <div className="bg-gray-800 p-4 w-full max-w-md border border-gray-700 rounded-lg">
            <div className="flex items-center justify-between mb-3">
              <h2 className="text-sm font-bold text-white">Nouvel environnement</h2>
              <button onClick={() => { setShowCreateModal(false); setFormError(''); }} className="text-gray-400 hover:text-white">
                <X className="w-4 h-4" />
              </button>
            </div>

            <form onSubmit={handleCreate} className="space-y-3">
              <div>
                <label className="block text-xs text-gray-400 mb-0.5">Nom *</label>
                <input
                  type="text"
                  value={formData.name}
                  onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                  className="w-full px-2 py-1.5 bg-gray-900 border border-gray-600 text-sm text-white focus:outline-none focus:border-blue-500"
                  placeholder="Mon environnement"
                  required
                />
              </div>

              <div>
                <label className="block text-xs text-gray-400 mb-0.5">Slug *</label>
                <input
                  type="text"
                  value={formData.slug}
                  onChange={(e) => setFormData({ ...formData, slug: e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, '') })}
                  className="w-full px-2 py-1.5 bg-gray-900 border border-gray-600 text-sm text-white font-mono focus:outline-none focus:border-blue-500"
                  placeholder="mon-env"
                  required
                />
              </div>

              <div>
                <label className="block text-xs text-gray-400 mb-0.5">Type</label>
                <select
                  value={formData.env_type}
                  onChange={(e) => setFormData({ ...formData, env_type: e.target.value })}
                  className="w-full px-2 py-1.5 bg-gray-900 border border-gray-600 text-sm text-white focus:outline-none focus:border-blue-500"
                >
                  <option value="dev">Developpement</option>
                  <option value="staging">Staging</option>
                  <option value="prod">Production</option>
                </select>
              </div>

              {formError && (
                <div className="px-3 py-2 bg-red-900/20 border border-red-600 text-red-400 text-sm">{formError}</div>
              )}

              <div className="flex gap-2 pt-1">
                <Button type="button" variant="secondary" onClick={() => { setShowCreateModal(false); setFormError(''); }} className="flex-1">
                  Annuler
                </Button>
                <Button type="submit" disabled={creating} loading={creating} className="flex-1">
                  <Check className="w-3.5 h-3.5" />
                  Creer
                </Button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}

export default Environments;
