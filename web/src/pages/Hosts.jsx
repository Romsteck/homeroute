import { useState, useEffect } from 'react';
import {
  HardDrive, Plus, Trash2, Edit, RefreshCw, Activity, X, Check,
  Power, Play, Square, RotateCw, Clock, ChevronDown, ChevronUp
} from 'lucide-react';
import Card from '../components/Card';
import Button from '../components/Button';
import StatusBadge from '../components/StatusBadge';
import PageHeader from '../components/PageHeader';
import Section from '../components/Section';
import {
  getHosts,
  addHost,
  updateHost,
  deleteHost,
  testHostConnection,
  refreshHostInterfaces,
  wakeHost,
  shutdownHost,
  rebootHost,
  getHostSchedules,
  addHostSchedule,
  deleteHostSchedule,
  toggleHostSchedule
} from '../api/client';
import useWebSocket from '../hooks/useWebSocket';

export default function Hosts() {
  const [hosts, setHosts] = useState([]);
  const [loading, setLoading] = useState(true);
  const [showAddModal, setShowAddModal] = useState(false);
  const [showScheduleModal, setShowScheduleModal] = useState(false);
  const [scheduleHostId, setScheduleHostId] = useState(null);
  const [expandedHost, setExpandedHost] = useState(null);

  // Add host form
  const [formData, setFormData] = useState({
    name: '',
    host: '',
    port: 22,
    username: 'root',
    password: '',
    groups: ''
  });
  const [addingHost, setAddingHost] = useState(false);
  const [addError, setAddError] = useState('');

  // Schedule form
  const [scheduleForm, setScheduleForm] = useState({
    action: 'wake',
    cron: '0 7 * * *',
    description: '',
    enabled: true
  });
  const [addingSchedule, setAddingSchedule] = useState(false);
  const [scheduleError, setScheduleError] = useState('');

  useWebSocket({
    'hosts:status': (data) => {
      setHosts(prev =>
        prev.map(h =>
          h.id === data.hostId
            ? { ...h, status: data.status, latency: data.latency, lastSeen: data.lastSeen }
            : h
        )
      );
    },
    // Legacy compat
    'servers:status': (data) => {
      setHosts(prev =>
        prev.map(h =>
          h.id === data.serverId
            ? { ...h, status: data.online ? 'online' : 'offline', latency: data.latency, lastSeen: data.lastSeen }
            : h
        )
      );
    }
  });

  useEffect(() => {
    loadHosts();
  }, []);

  const loadHosts = async () => {
    try {
      setLoading(true);
      const res = await getHosts();
      setHosts(res.data.hosts || []);
    } catch (error) {
      console.error('Failed to load hosts:', error);
    } finally {
      setLoading(false);
    }
  };

  // ── Host CRUD ───────────────────────────────

  const handleAddHost = async (e) => {
    e.preventDefault();
    setAddingHost(true);
    setAddError('');

    try {
      const groups = formData.groups
        .split(',')
        .map(g => g.trim())
        .filter(g => g);

      const res = await addHost({
        ...formData,
        groups,
        port: parseInt(formData.port)
      });

      if (res.data.success) {
        setHosts([...hosts, res.data.host]);
        setShowAddModal(false);
        resetForm();
      } else {
        setAddError(res.data.error || 'Failed to add host');
      }
    } catch (error) {
      setAddError(error.response?.data?.error || error.message || 'Failed to add host');
    } finally {
      setAddingHost(false);
    }
  };

  const handleDeleteHost = async (id) => {
    if (!confirm('Supprimer cet hote ?')) return;

    try {
      await deleteHost(id);
      setHosts(hosts.filter(h => h.id !== id));
    } catch (error) {
      alert('Failed to delete host: ' + error.message);
    }
  };

  const handleTestConnection = async (id) => {
    try {
      const res = await testHostConnection(id);
      if (res.data.success) {
        alert('Connexion reussie !');
      } else {
        alert('Echec : ' + res.data.error);
      }
    } catch (error) {
      alert('Echec : ' + error.message);
    }
  };

  const handleRefreshInterfaces = async (id) => {
    try {
      const res = await refreshHostInterfaces(id);
      if (res.data.success) {
        alert(`${res.data.interfaces.length} interface(s) detectee(s)`);
        loadHosts();
      }
    } catch (error) {
      alert('Echec : ' + error.message);
    }
  };

  // ── Power actions ───────────────────────────

  const handleWake = async (id) => {
    try {
      const res = await wakeHost(id);
      if (res.data.success) {
        alert('Paquet WOL envoye !');
      } else {
        alert('Echec : ' + res.data.error);
      }
    } catch (error) {
      alert('Echec WOL : ' + error.message);
    }
  };

  const handleShutdown = async (id) => {
    if (!confirm('Eteindre cet hote ?')) return;
    try {
      const res = await shutdownHost(id);
      if (res.data.success) alert('Commande envoyee !');
    } catch (error) {
      alert('Echec : ' + error.message);
    }
  };

  const handleReboot = async (id) => {
    if (!confirm('Redemarrer cet hote ?')) return;
    try {
      const res = await rebootHost(id);
      if (res.data.success) alert('Commande envoyee !');
    } catch (error) {
      alert('Echec : ' + error.message);
    }
  };

  // ── Schedules ───────────────────────────────

  const openScheduleModal = (hostId) => {
    setScheduleHostId(hostId);
    setShowScheduleModal(true);
  };

  const handleAddSchedule = async (e) => {
    e.preventDefault();
    setAddingSchedule(true);
    setScheduleError('');

    try {
      const res = await addHostSchedule(scheduleHostId, scheduleForm);
      if (res.data.success) {
        // Reload hosts to get updated schedules
        loadHosts();
        setShowScheduleModal(false);
        resetScheduleForm();
      } else {
        setScheduleError(res.data.error || 'Failed to add schedule');
      }
    } catch (error) {
      setScheduleError(error.response?.data?.error || error.message);
    } finally {
      setAddingSchedule(false);
    }
  };

  const handleDeleteSchedule = async (hostId, sid) => {
    if (!confirm('Supprimer ce planning ?')) return;
    try {
      await deleteHostSchedule(hostId, sid);
      loadHosts();
    } catch (error) {
      alert('Echec : ' + error.message);
    }
  };

  const handleToggleSchedule = async (hostId, sid) => {
    try {
      await toggleHostSchedule(hostId, sid);
      loadHosts();
    } catch (error) {
      alert('Echec : ' + error.message);
    }
  };

  // ── Form helpers ────────────────────────────

  const resetForm = () => {
    setFormData({ name: '', host: '', port: 22, username: 'root', password: '', groups: '' });
    setAddError('');
  };

  const resetScheduleForm = () => {
    setScheduleForm({ action: 'wake', cron: '0 7 * * *', description: '', enabled: true });
    setScheduleError('');
    setScheduleHostId(null);
  };

  const getStatusColor = (status) => {
    switch (status) {
      case 'online': return 'success';
      case 'offline': return 'danger';
      default: return 'secondary';
    }
  };

  const getActionColor = (action) => {
    switch (action) {
      case 'wake': return 'bg-green-600/20 text-green-400';
      case 'shutdown': return 'bg-red-600/20 text-red-400';
      case 'reboot': return 'bg-yellow-600/20 text-yellow-400';
      default: return 'bg-blue-600/20 text-blue-400';
    }
  };

  return (
    <div>
      <PageHeader title="Hotes" icon={HardDrive}>
        <Button onClick={() => setShowAddModal(true)}>
          <Plus className="w-4 h-4 mr-2" />
          Ajouter
        </Button>
      </PageHeader>

      <Section title="Liste des hotes">
        {loading ? (
          <div className="text-center py-12 text-gray-400">Chargement...</div>
        ) : hosts.length === 0 ? (
          <Card title="Aucun hote" icon={HardDrive}>
            <p className="text-gray-400">
              Aucun hote configure. Cliquez sur "Ajouter" pour commencer.
            </p>
          </Card>
        ) : (
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
            {hosts.map((host) => (
              <Card key={host.id} title={host.name} icon={HardDrive}>
                <div className="space-y-3">
                  {/* Status row */}
                  <div className="flex items-center justify-between">
                    <StatusBadge status={getStatusColor(host.status)}>
                      {host.status || 'unknown'}
                    </StatusBadge>
                    {host.latency > 0 && (
                      <span className="text-sm text-gray-400">{host.latency}ms</span>
                    )}
                  </div>

                  {/* Host details */}
                  <div className="space-y-1 text-sm">
                    <div className="flex justify-between">
                      <span className="text-gray-400">Adresse :</span>
                      <span className="text-white font-mono">{host.host}:{host.port}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-400">Utilisateur :</span>
                      <span className="text-white">{host.username}</span>
                    </div>
                    {host.mac && (
                      <div className="flex justify-between">
                        <span className="text-gray-400">MAC :</span>
                        <span className="text-white font-mono text-xs">{host.mac}</span>
                      </div>
                    )}
                    {host.interface && (
                      <div className="flex justify-between">
                        <span className="text-gray-400">Interface :</span>
                        <span className="text-white font-mono text-xs">{host.interface}</span>
                      </div>
                    )}
                  </div>

                  {/* Groups */}
                  {host.groups && host.groups.length > 0 && (
                    <div className="flex flex-wrap gap-1">
                      {host.groups.map((group, idx) => (
                        <span key={idx} className="px-2 py-0.5 text-xs bg-blue-600/20 text-blue-400">
                          {group}
                        </span>
                      ))}
                    </div>
                  )}

                  {host.lastSeen && (
                    <div className="text-xs text-gray-500">
                      Vu : {new Date(host.lastSeen).toLocaleString()}
                    </div>
                  )}

                  {/* Power controls */}
                  <div className="flex gap-2 pt-2 border-t border-gray-700">
                    <Button
                      variant="success"
                      onClick={() => handleWake(host.id)}
                      disabled={host.status === 'online'}
                      className="flex-1 text-xs"
                    >
                      <Play className="w-3 h-3 mr-1" />
                      Wake
                    </Button>
                    <Button
                      variant="warning"
                      onClick={() => handleReboot(host.id)}
                      disabled={host.status !== 'online'}
                      className="flex-1 text-xs"
                    >
                      <RotateCw className="w-3 h-3 mr-1" />
                      Reboot
                    </Button>
                    <Button
                      variant="danger"
                      onClick={() => handleShutdown(host.id)}
                      disabled={host.status !== 'online'}
                      className="flex-1 text-xs"
                    >
                      <Square className="w-3 h-3 mr-1" />
                      Shutdown
                    </Button>
                  </div>

                  {/* Management actions */}
                  <div className="flex gap-2">
                    <Button
                      variant="secondary"
                      onClick={() => handleTestConnection(host.id)}
                      className="flex-1 text-xs"
                    >
                      <Activity className="w-3 h-3 mr-1" />
                      Test
                    </Button>
                    <Button
                      variant="secondary"
                      onClick={() => handleRefreshInterfaces(host.id)}
                      className="flex-1 text-xs"
                    >
                      <RefreshCw className="w-3 h-3 mr-1" />
                      Interfaces
                    </Button>
                    <Button
                      variant="danger"
                      onClick={() => handleDeleteHost(host.id)}
                      className="text-xs"
                    >
                      <Trash2 className="w-3 h-3" />
                    </Button>
                  </div>

                  {/* Schedules toggle */}
                  <div>
                    <button
                      onClick={() => setExpandedHost(expandedHost === host.id ? null : host.id)}
                      className="flex items-center gap-1 text-sm text-gray-400 hover:text-white transition-colors w-full"
                    >
                      {expandedHost === host.id ? (
                        <ChevronUp className="w-4 h-4" />
                      ) : (
                        <ChevronDown className="w-4 h-4" />
                      )}
                      <Clock className="w-3 h-3" />
                      Plannings ({(host.schedules || []).length})
                    </button>

                    {expandedHost === host.id && (
                      <div className="mt-2 space-y-2">
                        {(host.schedules || []).length === 0 ? (
                          <p className="text-xs text-gray-500">Aucun planning</p>
                        ) : (
                          (host.schedules || []).map((schedule) => (
                            <div
                              key={schedule.id}
                              className="flex items-center justify-between bg-gray-700/30 p-2 text-xs"
                            >
                              <div className="flex items-center gap-2 flex-1">
                                <span className={`px-1.5 py-0.5 ${getActionColor(schedule.action)}`}>
                                  {schedule.action}
                                </span>
                                <span className="text-gray-300 font-mono">{schedule.cron}</span>
                                {schedule.description && (
                                  <span className="text-gray-500 truncate">{schedule.description}</span>
                                )}
                              </div>
                              <div className="flex items-center gap-1">
                                <label className="flex items-center gap-1 cursor-pointer">
                                  <input
                                    type="checkbox"
                                    checked={schedule.enabled}
                                    onChange={() => handleToggleSchedule(host.id, schedule.id)}
                                    className="w-3 h-3"
                                  />
                                </label>
                                <button
                                  onClick={() => handleDeleteSchedule(host.id, schedule.id)}
                                  className="text-gray-500 hover:text-red-400"
                                >
                                  <Trash2 className="w-3 h-3" />
                                </button>
                              </div>
                            </div>
                          ))
                        )}
                        <Button
                          variant="secondary"
                          onClick={() => openScheduleModal(host.id)}
                          className="text-xs w-full"
                        >
                          <Plus className="w-3 h-3 mr-1" />
                          Ajouter planning
                        </Button>
                      </div>
                    )}
                  </div>
                </div>
              </Card>
            ))}
          </div>
        )}
      </Section>

      {/* Add Host Modal */}
      {showAddModal && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <div className="bg-gray-800 p-6 w-full max-w-md">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-bold text-white">Ajouter un hote</h2>
              <button onClick={() => { setShowAddModal(false); resetForm(); }} className="text-gray-400 hover:text-white">
                <X className="w-5 h-5" />
              </button>
            </div>

            <form onSubmit={handleAddHost} className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-gray-300 mb-1">Nom *</label>
                <input
                  type="text"
                  value={formData.name}
                  onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-700 border border-gray-600 text-white focus:ring-2 focus:ring-blue-500"
                  placeholder="Mon serveur"
                  required
                />
              </div>

              <div className="grid grid-cols-3 gap-2">
                <div className="col-span-2">
                  <label className="block text-sm font-medium text-gray-300 mb-1">Adresse IP *</label>
                  <input
                    type="text"
                    value={formData.host}
                    onChange={(e) => setFormData({ ...formData, host: e.target.value })}
                    className="w-full px-3 py-2 bg-gray-700 border border-gray-600 text-white focus:ring-2 focus:ring-blue-500"
                    placeholder="10.0.0.10"
                    required
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-gray-300 mb-1">Port</label>
                  <input
                    type="number"
                    value={formData.port}
                    onChange={(e) => setFormData({ ...formData, port: e.target.value })}
                    className="w-full px-3 py-2 bg-gray-700 border border-gray-600 text-white focus:ring-2 focus:ring-blue-500"
                    placeholder="22"
                  />
                </div>
              </div>

              <div>
                <label className="block text-sm font-medium text-gray-300 mb-1">Utilisateur SSH *</label>
                <input
                  type="text"
                  value={formData.username}
                  onChange={(e) => setFormData({ ...formData, username: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-700 border border-gray-600 text-white focus:ring-2 focus:ring-blue-500"
                  placeholder="root"
                  required
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-gray-300 mb-1">Mot de passe SSH *</label>
                <input
                  type="password"
                  value={formData.password}
                  onChange={(e) => setFormData({ ...formData, password: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-700 border border-gray-600 text-white focus:ring-2 focus:ring-blue-500"
                  placeholder="••••••••"
                  required
                />
                <p className="text-xs text-gray-400 mt-1">Utilise une seule fois pour configurer l'authentification par cle SSH</p>
              </div>

              <div>
                <label className="block text-sm font-medium text-gray-300 mb-1">Groupes (virgule)</label>
                <input
                  type="text"
                  value={formData.groups}
                  onChange={(e) => setFormData({ ...formData, groups: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-700 border border-gray-600 text-white focus:ring-2 focus:ring-blue-500"
                  placeholder="storage, critical"
                />
              </div>

              {addError && (
                <div className="p-3 bg-red-900/20 border border-red-600 text-red-400 text-sm">{addError}</div>
              )}

              <div className="flex gap-2 pt-2">
                <Button type="button" variant="secondary" onClick={() => { setShowAddModal(false); resetForm(); }} className="flex-1">
                  Annuler
                </Button>
                <Button type="submit" disabled={addingHost} className="flex-1">
                  {addingHost ? (
                    <><RefreshCw className="w-4 h-4 mr-2 animate-spin" />Ajout...</>
                  ) : (
                    <><Check className="w-4 h-4 mr-2" />Ajouter</>
                  )}
                </Button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Add Schedule Modal */}
      {showScheduleModal && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <div className="bg-gray-800 p-6 w-full max-w-md">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-bold text-white">Ajouter un planning</h2>
              <button
                onClick={() => { setShowScheduleModal(false); resetScheduleForm(); }}
                className="text-gray-400 hover:text-white"
              >
                <X className="w-5 h-5" />
              </button>
            </div>

            <form onSubmit={handleAddSchedule} className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-gray-300 mb-1">Action *</label>
                <select
                  value={scheduleForm.action}
                  onChange={(e) => setScheduleForm({ ...scheduleForm, action: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-700 border border-gray-600 text-white focus:ring-2 focus:ring-blue-500"
                  required
                >
                  <option value="wake">Wake</option>
                  <option value="shutdown">Shutdown</option>
                  <option value="reboot">Reboot</option>
                </select>
              </div>

              <div>
                <label className="block text-sm font-medium text-gray-300 mb-1">Expression cron *</label>
                <input
                  type="text"
                  value={scheduleForm.cron}
                  onChange={(e) => setScheduleForm({ ...scheduleForm, cron: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-700 border border-gray-600 text-white font-mono focus:ring-2 focus:ring-blue-500"
                  placeholder="0 7 * * *"
                  required
                />
                <p className="text-xs text-gray-400 mt-1">
                  Format : minute heure jour mois jour_semaine (ex: "0 7 * * *" = chaque jour a 7h)
                </p>
              </div>

              <div>
                <label className="block text-sm font-medium text-gray-300 mb-1">Description</label>
                <input
                  type="text"
                  value={scheduleForm.description}
                  onChange={(e) => setScheduleForm({ ...scheduleForm, description: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-700 border border-gray-600 text-white focus:ring-2 focus:ring-blue-500"
                  placeholder="Reveil quotidien"
                />
              </div>

              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="schedule-enabled"
                  checked={scheduleForm.enabled}
                  onChange={(e) => setScheduleForm({ ...scheduleForm, enabled: e.target.checked })}
                  className="w-4 h-4"
                />
                <label htmlFor="schedule-enabled" className="text-sm text-gray-300 cursor-pointer">
                  Activer immediatement
                </label>
              </div>

              {scheduleError && (
                <div className="p-3 bg-red-900/20 border border-red-600 text-red-400 text-sm">{scheduleError}</div>
              )}

              <div className="flex gap-2 pt-2">
                <Button type="button" variant="secondary" onClick={() => { setShowScheduleModal(false); resetScheduleForm(); }} className="flex-1">
                  Annuler
                </Button>
                <Button type="submit" disabled={addingSchedule} className="flex-1">
                  {addingSchedule ? (
                    <><RotateCw className="w-4 h-4 mr-2 animate-spin" />Ajout...</>
                  ) : (
                    <><Check className="w-4 h-4 mr-2" />Ajouter</>
                  )}
                </Button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}
