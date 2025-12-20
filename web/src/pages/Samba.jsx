import { useState, useEffect } from 'react';
import {
  FolderOpen,
  Plus,
  Trash2,
  Settings,
  RefreshCw,
  CheckCircle,
  XCircle,
  Power,
  Users,
  Monitor,
  FileText,
  Pencil,
  Eye,
  EyeOff,
  UserPlus,
  Key,
  Play,
  Server,
  Download
} from 'lucide-react';
import Card from '../components/Card';
import Button from '../components/Button';
import {
  getSambaConfig,
  getSambaStatus,
  getSambaShares,
  addSambaShare,
  updateSambaShare,
  deleteSambaShare,
  toggleSambaShare,
  applySambaConfig,
  restartSamba,
  getSambaSessions,
  getSambaOpenFiles,
  getSambaUsers,
  addSambaUser,
  deleteSambaUser,
  changeSambaUserPassword,
  enableSambaUser,
  disableSambaUser,
  importSambaShares
} from '../api/client';

function Samba() {
  const [config, setConfig] = useState(null);
  const [status, setStatus] = useState(null);
  const [shares, setShares] = useState([]);
  const [users, setUsers] = useState([]);
  const [sessions, setSessions] = useState([]);
  const [openFiles, setOpenFiles] = useState([]);
  const [loading, setLoading] = useState(true);
  const [message, setMessage] = useState(null);
  const [activeTab, setActiveTab] = useState('shares');
  const [pendingChanges, setPendingChanges] = useState(false);

  // Modal states
  const [showAddShareModal, setShowAddShareModal] = useState(false);
  const [showEditShareModal, setShowEditShareModal] = useState(false);
  const [showAddUserModal, setShowAddUserModal] = useState(false);
  const [showPasswordModal, setShowPasswordModal] = useState(false);
  const [editingShare, setEditingShare] = useState(null);
  const [editingUser, setEditingUser] = useState(null);

  // Form states
  const [newShare, setNewShare] = useState({
    name: '',
    path: '',
    comment: '',
    browseable: true,
    writable: false,
    guestOk: false,
    validUsers: '',
    writeList: ''
  });
  const [editShareForm, setEditShareForm] = useState({});
  const [newUser, setNewUser] = useState({ username: '', password: '', confirmPassword: '' });
  const [newPassword, setNewPassword] = useState({ password: '', confirmPassword: '' });
  const [showPassword, setShowPassword] = useState(false);

  // Action states
  const [saving, setSaving] = useState(false);
  const [applying, setApplying] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [importing, setImporting] = useState(false);

  useEffect(() => {
    fetchData();
  }, []);

  useEffect(() => {
    // Refresh sessions and files periodically when on those tabs
    if (activeTab === 'sessions' || activeTab === 'files') {
      const interval = setInterval(() => {
        if (activeTab === 'sessions') fetchSessions();
        if (activeTab === 'files') fetchFiles();
      }, 10000);
      return () => clearInterval(interval);
    }
  }, [activeTab]);

  async function fetchData() {
    try {
      const [configRes, statusRes, sharesRes, usersRes, sessionsRes, filesRes] = await Promise.all([
        getSambaConfig(),
        getSambaStatus(),
        getSambaShares(),
        getSambaUsers(),
        getSambaSessions(),
        getSambaOpenFiles()
      ]);

      if (configRes.data.success) setConfig(configRes.data.config);
      if (statusRes.data.success) setStatus(statusRes.data.status);
      if (sharesRes.data.success) setShares(sharesRes.data.shares || []);
      if (usersRes.data.success) setUsers(usersRes.data.users || []);
      if (sessionsRes.data.success) setSessions(sessionsRes.data.sessions || []);
      if (filesRes.data.success) setOpenFiles(filesRes.data.files || []);
    } catch (error) {
      console.error('Error:', error);
      setMessage({ type: 'error', text: 'Erreur de chargement' });
    } finally {
      setLoading(false);
    }
  }

  async function fetchSessions() {
    try {
      const res = await getSambaSessions();
      if (res.data.success) setSessions(res.data.sessions || []);
    } catch (error) {
      console.error('Error fetching sessions:', error);
    }
  }

  async function fetchFiles() {
    try {
      const res = await getSambaOpenFiles();
      if (res.data.success) setOpenFiles(res.data.files || []);
    } catch (error) {
      console.error('Error fetching files:', error);
    }
  }

  // ========== Share Handlers ==========

  async function handleAddShare() {
    if (!newShare.name || !newShare.path) {
      setMessage({ type: 'error', text: 'Le nom et le chemin sont requis' });
      return;
    }

    setSaving(true);
    try {
      const payload = {
        ...newShare,
        validUsers: newShare.validUsers ? newShare.validUsers.split(',').map(s => s.trim()).filter(Boolean) : [],
        writeList: newShare.writeList ? newShare.writeList.split(',').map(s => s.trim()).filter(Boolean) : []
      };

      const res = await addSambaShare(payload);
      if (res.data.success) {
        setMessage({ type: 'success', text: res.data.message || 'Partage ajouté' });
        setShowAddShareModal(false);
        setNewShare({ name: '', path: '', comment: '', browseable: true, writable: false, guestOk: false, validUsers: '', writeList: '' });
        setPendingChanges(true);
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: error.response?.data?.error || 'Erreur' });
    } finally {
      setSaving(false);
    }
  }

  function openEditShareModal(share) {
    setEditingShare(share);
    setEditShareForm({
      comment: share.comment || '',
      path: share.path,
      browseable: share.browseable,
      writable: share.writable,
      guestOk: share.guestOk,
      validUsers: (share.validUsers || []).join(', '),
      writeList: (share.writeList || []).join(', ')
    });
    setShowEditShareModal(true);
  }

  async function handleEditShare() {
    if (!editShareForm.path) {
      setMessage({ type: 'error', text: 'Le chemin est requis' });
      return;
    }

    setSaving(true);
    try {
      const payload = {
        ...editShareForm,
        validUsers: editShareForm.validUsers ? editShareForm.validUsers.split(',').map(s => s.trim()).filter(Boolean) : [],
        writeList: editShareForm.writeList ? editShareForm.writeList.split(',').map(s => s.trim()).filter(Boolean) : []
      };

      const res = await updateSambaShare(editingShare.id, payload);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Partage modifié' });
        setShowEditShareModal(false);
        setEditingShare(null);
        setPendingChanges(true);
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: error.response?.data?.error || 'Erreur' });
    } finally {
      setSaving(false);
    }
  }

  async function handleToggleShare(shareId, enabled) {
    try {
      const res = await toggleSambaShare(shareId, enabled);
      if (res.data.success) {
        setPendingChanges(true);
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: 'Erreur' });
    }
  }

  async function handleDeleteShare(shareId) {
    if (!confirm('Supprimer ce partage ?')) return;
    try {
      const res = await deleteSambaShare(shareId);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Partage supprimé' });
        setPendingChanges(true);
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: 'Erreur' });
    }
  }

  async function handleApplyConfig() {
    setApplying(true);
    setMessage(null);
    try {
      const res = await applySambaConfig();
      if (res.data.success) {
        setMessage({ type: 'success', text: res.data.message || 'Configuration appliquée' });
        setPendingChanges(false);
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: error.response?.data?.error || 'Erreur' });
    } finally {
      setApplying(false);
    }
  }

  async function handleRestartSamba() {
    setRestarting(true);
    try {
      const res = await restartSamba();
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Samba redémarré' });
        setTimeout(fetchData, 2000);
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: 'Erreur' });
    } finally {
      setRestarting(false);
    }
  }

  async function handleImportShares() {
    setImporting(true);
    setMessage(null);
    try {
      const res = await importSambaShares();
      if (res.data.success) {
        if (res.data.imported > 0) {
          setMessage({ type: 'success', text: res.data.message });
          fetchData();
        } else {
          setMessage({ type: 'info', text: res.data.message || 'Aucun nouveau partage à importer' });
        }
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: error.response?.data?.error || 'Erreur d\'import' });
    } finally {
      setImporting(false);
    }
  }

  // ========== User Handlers ==========

  async function handleAddUser() {
    if (!newUser.username || !newUser.password) {
      setMessage({ type: 'error', text: 'Nom d\'utilisateur et mot de passe requis' });
      return;
    }
    if (newUser.password !== newUser.confirmPassword) {
      setMessage({ type: 'error', text: 'Les mots de passe ne correspondent pas' });
      return;
    }
    if (newUser.password.length < 8) {
      setMessage({ type: 'error', text: 'Le mot de passe doit contenir au moins 8 caractères' });
      return;
    }

    setSaving(true);
    try {
      const res = await addSambaUser(newUser.username, newUser.password);
      if (res.data.success) {
        setMessage({ type: 'success', text: res.data.message || 'Utilisateur créé' });
        setShowAddUserModal(false);
        setNewUser({ username: '', password: '', confirmPassword: '' });
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: error.response?.data?.error || 'Erreur' });
    } finally {
      setSaving(false);
    }
  }

  async function handleDeleteUser(username) {
    if (!confirm(`Supprimer l'utilisateur Samba "${username}" ?`)) return;
    try {
      const res = await deleteSambaUser(username);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Utilisateur supprimé' });
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: 'Erreur' });
    }
  }

  async function handleToggleUser(username, enabled) {
    try {
      const res = enabled ? await enableSambaUser(username) : await disableSambaUser(username);
      if (res.data.success) {
        setMessage({ type: 'success', text: res.data.message });
        fetchData();
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: 'Erreur' });
    }
  }

  function openPasswordModal(user) {
    setEditingUser(user);
    setNewPassword({ password: '', confirmPassword: '' });
    setShowPasswordModal(true);
  }

  async function handleChangePassword() {
    if (!newPassword.password) {
      setMessage({ type: 'error', text: 'Mot de passe requis' });
      return;
    }
    if (newPassword.password !== newPassword.confirmPassword) {
      setMessage({ type: 'error', text: 'Les mots de passe ne correspondent pas' });
      return;
    }
    if (newPassword.password.length < 8) {
      setMessage({ type: 'error', text: 'Le mot de passe doit contenir au moins 8 caractères' });
      return;
    }

    setSaving(true);
    try {
      const res = await changeSambaUserPassword(editingUser.username, newPassword.password);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Mot de passe modifié' });
        setShowPasswordModal(false);
        setEditingUser(null);
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: error.response?.data?.error || 'Erreur' });
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-400"></div>
      </div>
    );
  }

  const smbdRunning = status?.smbd?.active;
  const nmbdRunning = status?.nmbd?.active;
  const activeShares = shares.filter(s => s.enabled).length;

  const tabs = [
    { id: 'shares', label: 'Partages', icon: FolderOpen },
    { id: 'users', label: 'Utilisateurs', icon: Users },
    { id: 'sessions', label: 'Sessions', icon: Monitor },
    { id: 'files', label: 'Fichiers ouverts', icon: FileText }
  ];

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Serveur Samba</h1>
        <div className="flex gap-2">
          <Button onClick={handleRestartSamba} loading={restarting} variant="secondary">
            <RefreshCw className="w-4 h-4" />
            Redémarrer
          </Button>
          {pendingChanges && (
            <Button onClick={handleApplyConfig} loading={applying} variant="primary">
              <Play className="w-4 h-4" />
              Appliquer
            </Button>
          )}
        </div>
      </div>

      {/* Message */}
      {message && (
        <div className={`p-4 rounded-lg flex items-center gap-2 ${
          message.type === 'success' ? 'bg-green-900/50 text-green-400' :
          message.type === 'info' ? 'bg-blue-900/50 text-blue-400' : 'bg-red-900/50 text-red-400'
        }`}>
          {message.type === 'success' ? <CheckCircle className="w-5 h-5" /> :
           message.type === 'info' ? <Settings className="w-5 h-5" /> : <XCircle className="w-5 h-5" />}
          {message.text}
        </div>
      )}

      {/* Pending Changes Warning */}
      {pendingChanges && (
        <div className="p-4 rounded-lg bg-yellow-900/30 border border-yellow-700 text-yellow-400 flex items-center gap-2">
          <Settings className="w-5 h-5" />
          <span>Des modifications sont en attente. Cliquez sur "Appliquer" pour les activer.</span>
        </div>
      )}

      {/* Status Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <Card title="smbd" icon={Server}>
          <div className="flex items-center gap-2">
            <div className={`w-3 h-3 rounded-full ${smbdRunning ? 'bg-green-400' : 'bg-red-400'}`} />
            <span className={smbdRunning ? 'text-green-400' : 'text-red-400'}>
              {smbdRunning ? 'En ligne' : 'Hors ligne'}
            </span>
          </div>
          {status?.smbd?.pid && (
            <p className="text-xs text-gray-500 mt-1">PID: {status.smbd.pid}</p>
          )}
        </Card>

        <Card title="nmbd" icon={Server}>
          <div className="flex items-center gap-2">
            <div className={`w-3 h-3 rounded-full ${nmbdRunning ? 'bg-green-400' : 'bg-red-400'}`} />
            <span className={nmbdRunning ? 'text-green-400' : 'text-red-400'}>
              {nmbdRunning ? 'En ligne' : 'Hors ligne'}
            </span>
          </div>
          {status?.nmbd?.pid && (
            <p className="text-xs text-gray-500 mt-1">PID: {status.nmbd.pid}</p>
          )}
        </Card>

        <Card title="Sessions actives" icon={Monitor}>
          <div className="text-2xl font-bold text-blue-400">
            {sessions.length}
          </div>
        </Card>

        <Card title="Partages" icon={FolderOpen}>
          <div className="text-2xl font-bold text-green-400">
            {activeShares} / {shares.length}
          </div>
          <p className="text-xs text-gray-500 mt-1">actifs</p>
        </Card>
      </div>

      {/* Tabs */}
      <div className="flex gap-1 border-b border-gray-700">
        {tabs.map(tab => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`flex items-center gap-2 px-4 py-2 text-sm font-medium transition-colors border-b-2 -mb-px ${
              activeTab === tab.id
                ? 'border-blue-500 text-blue-400'
                : 'border-transparent text-gray-400 hover:text-gray-300'
            }`}
          >
            <tab.icon className="w-4 h-4" />
            {tab.label}
          </button>
        ))}
      </div>

      {/* Tab Content */}
      <div className="min-h-[400px]">
        {/* Shares Tab */}
        {activeTab === 'shares' && (
          <Card
            title="Partages configurés"
            icon={FolderOpen}
            actions={
              <div className="flex gap-2">
                <Button onClick={handleImportShares} loading={importing} variant="secondary">
                  <Download className="w-4 h-4" />
                  Importer
                </Button>
                <Button onClick={() => setShowAddShareModal(true)}>
                  <Plus className="w-4 h-4" />
                  Ajouter
                </Button>
              </div>
            }
          >
            {shares.length === 0 ? (
              <div className="text-center py-8 text-gray-500">
                <FolderOpen className="w-12 h-12 mx-auto mb-2 opacity-50" />
                <p>Aucun partage configuré</p>
              </div>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="text-left text-gray-400 border-b border-gray-700">
                      <th className="pb-2">Nom</th>
                      <th className="pb-2">Chemin</th>
                      <th className="pb-2">Permissions</th>
                      <th className="pb-2">Status</th>
                      <th className="pb-2 text-right">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {shares.map(share => (
                      <tr key={share.id} className="border-b border-gray-700/50">
                        <td className="py-3">
                          <div className="font-medium">{share.name}</div>
                          {share.comment && (
                            <div className="text-xs text-gray-500">{share.comment}</div>
                          )}
                        </td>
                        <td className="py-3 font-mono text-sm text-gray-300">
                          {share.path}
                        </td>
                        <td className="py-3">
                          <div className="flex gap-1 flex-wrap">
                            {share.writable && (
                              <span className="px-1.5 py-0.5 bg-blue-900/50 text-blue-400 text-xs rounded">RW</span>
                            )}
                            {!share.writable && (
                              <span className="px-1.5 py-0.5 bg-gray-700 text-gray-400 text-xs rounded">RO</span>
                            )}
                            {share.guestOk && (
                              <span className="px-1.5 py-0.5 bg-yellow-900/50 text-yellow-400 text-xs rounded">Guest</span>
                            )}
                            {share.browseable && (
                              <span className="px-1.5 py-0.5 bg-green-900/50 text-green-400 text-xs rounded">Visible</span>
                            )}
                          </div>
                        </td>
                        <td className="py-3">
                          <button
                            onClick={() => handleToggleShare(share.id, !share.enabled)}
                            className={`p-1.5 rounded transition-colors ${
                              share.enabled
                                ? 'text-green-400 bg-green-900/30 hover:bg-green-900/50'
                                : 'text-gray-500 bg-gray-700/30 hover:bg-gray-700/50'
                            }`}
                            title={share.enabled ? 'Désactiver' : 'Activer'}
                          >
                            <Power className="w-4 h-4" />
                          </button>
                        </td>
                        <td className="py-3 text-right">
                          <div className="flex justify-end gap-1">
                            <button
                              onClick={() => openEditShareModal(share)}
                              className="text-blue-400 hover:text-blue-300 p-1"
                              title="Modifier"
                            >
                              <Pencil className="w-4 h-4" />
                            </button>
                            <button
                              onClick={() => handleDeleteShare(share.id)}
                              className="text-red-400 hover:text-red-300 p-1"
                              title="Supprimer"
                            >
                              <Trash2 className="w-4 h-4" />
                            </button>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </Card>
        )}

        {/* Users Tab */}
        {activeTab === 'users' && (
          <Card
            title="Utilisateurs Samba"
            icon={Users}
            actions={
              <Button onClick={() => setShowAddUserModal(true)}>
                <UserPlus className="w-4 h-4" />
                Ajouter
              </Button>
            }
          >
            {users.length === 0 ? (
              <div className="text-center py-8 text-gray-500">
                <Users className="w-12 h-12 mx-auto mb-2 opacity-50" />
                <p>Aucun utilisateur Samba</p>
                <p className="text-xs mt-1">Les utilisateurs doivent d'abord exister dans le système (useradd)</p>
              </div>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="text-left text-gray-400 border-b border-gray-700">
                      <th className="pb-2">Utilisateur</th>
                      <th className="pb-2">Nom complet</th>
                      <th className="pb-2">Status</th>
                      <th className="pb-2 text-right">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {users.map(user => (
                      <tr key={user.username} className="border-b border-gray-700/50">
                        <td className="py-3 font-medium">{user.username}</td>
                        <td className="py-3 text-gray-400">{user.fullName || '-'}</td>
                        <td className="py-3">
                          <button
                            onClick={() => handleToggleUser(user.username, !user.enabled)}
                            className={`p-1.5 rounded transition-colors ${
                              user.enabled
                                ? 'text-green-400 bg-green-900/30 hover:bg-green-900/50'
                                : 'text-gray-500 bg-gray-700/30 hover:bg-gray-700/50'
                            }`}
                            title={user.enabled ? 'Désactiver' : 'Activer'}
                          >
                            <Power className="w-4 h-4" />
                          </button>
                        </td>
                        <td className="py-3 text-right">
                          <div className="flex justify-end gap-1">
                            <button
                              onClick={() => openPasswordModal(user)}
                              className="text-yellow-400 hover:text-yellow-300 p-1"
                              title="Changer le mot de passe"
                            >
                              <Key className="w-4 h-4" />
                            </button>
                            <button
                              onClick={() => handleDeleteUser(user.username)}
                              className="text-red-400 hover:text-red-300 p-1"
                              title="Supprimer"
                            >
                              <Trash2 className="w-4 h-4" />
                            </button>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </Card>
        )}

        {/* Sessions Tab */}
        {activeTab === 'sessions' && (
          <Card
            title="Sessions actives"
            icon={Monitor}
            actions={
              <Button onClick={fetchSessions} variant="secondary">
                <RefreshCw className="w-4 h-4" />
                Actualiser
              </Button>
            }
          >
            {sessions.length === 0 ? (
              <div className="text-center py-8 text-gray-500">
                <Monitor className="w-12 h-12 mx-auto mb-2 opacity-50" />
                <p>Aucune session active</p>
              </div>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="text-left text-gray-400 border-b border-gray-700">
                      <th className="pb-2">PID</th>
                      <th className="pb-2">Utilisateur</th>
                      <th className="pb-2">Groupe</th>
                      <th className="pb-2">Machine</th>
                      <th className="pb-2">Protocole</th>
                    </tr>
                  </thead>
                  <tbody>
                    {sessions.map((session, idx) => (
                      <tr key={idx} className="border-b border-gray-700/50">
                        <td className="py-3 font-mono text-gray-400">{session.pid}</td>
                        <td className="py-3 font-medium">{session.username}</td>
                        <td className="py-3 text-gray-400">{session.group}</td>
                        <td className="py-3 font-mono text-blue-400">{session.machine}</td>
                        <td className="py-3">
                          <span className="px-2 py-0.5 bg-gray-700 rounded text-xs">
                            {session.protocol || 'SMB'}
                          </span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </Card>
        )}

        {/* Files Tab */}
        {activeTab === 'files' && (
          <Card
            title="Fichiers ouverts"
            icon={FileText}
            actions={
              <Button onClick={fetchFiles} variant="secondary">
                <RefreshCw className="w-4 h-4" />
                Actualiser
              </Button>
            }
          >
            {openFiles.length === 0 ? (
              <div className="text-center py-8 text-gray-500">
                <FileText className="w-12 h-12 mx-auto mb-2 opacity-50" />
                <p>Aucun fichier ouvert</p>
              </div>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="text-left text-gray-400 border-b border-gray-700">
                      <th className="pb-2">Fichier</th>
                      <th className="pb-2">Partage</th>
                      <th className="pb-2">PID</th>
                      <th className="pb-2">Acces</th>
                    </tr>
                  </thead>
                  <tbody>
                    {openFiles.map((file, idx) => (
                      <tr key={idx} className="border-b border-gray-700/50">
                        <td className="py-3 font-mono text-sm">{file.name}</td>
                        <td className="py-3 text-gray-400">{file.sharePath}</td>
                        <td className="py-3 font-mono text-gray-400">{file.pid}</td>
                        <td className="py-3">
                          <span className={`px-2 py-0.5 rounded text-xs ${
                            file.rwAccess?.includes('W') ? 'bg-blue-900/50 text-blue-400' : 'bg-gray-700 text-gray-400'
                          }`}>
                            {file.rwAccess || 'RO'}
                          </span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </Card>
        )}
      </div>

      {/* Add Share Modal */}
      {showAddShareModal && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
          <div className="bg-gray-800 rounded-lg p-6 w-full max-w-lg border border-gray-700 max-h-[90vh] overflow-y-auto">
            <h2 className="text-xl font-bold mb-4 flex items-center gap-2">
              <FolderOpen className="w-5 h-5 text-blue-400" />
              Nouveau partage
            </h2>

            <div className="space-y-4">
              <div>
                <label className="block text-sm text-gray-400 mb-1">Nom du partage *</label>
                <input
                  type="text"
                  placeholder="documents"
                  value={newShare.name}
                  onChange={e => setNewShare({ ...newShare, name: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
                <p className="text-xs text-gray-500 mt-1">Lettres, chiffres, tirets et underscores uniquement</p>
              </div>

              <div>
                <label className="block text-sm text-gray-400 mb-1">Chemin local *</label>
                <input
                  type="text"
                  placeholder="/srv/samba/documents"
                  value={newShare.path}
                  onChange={e => setNewShare({ ...newShare, path: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
              </div>

              <div>
                <label className="block text-sm text-gray-400 mb-1">Description</label>
                <input
                  type="text"
                  placeholder="Documents partagés"
                  value={newShare.comment}
                  onChange={e => setNewShare({ ...newShare, comment: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
              </div>

              <div className="grid grid-cols-3 gap-4">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={newShare.browseable}
                    onChange={e => setNewShare({ ...newShare, browseable: e.target.checked })}
                    className="rounded bg-gray-700 border-gray-600"
                  />
                  <span className="text-sm">Visible</span>
                </label>
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={newShare.writable}
                    onChange={e => setNewShare({ ...newShare, writable: e.target.checked })}
                    className="rounded bg-gray-700 border-gray-600"
                  />
                  <span className="text-sm">Ecriture</span>
                </label>
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={newShare.guestOk}
                    onChange={e => setNewShare({ ...newShare, guestOk: e.target.checked })}
                    className="rounded bg-gray-700 border-gray-600"
                  />
                  <span className="text-sm">Guest OK</span>
                </label>
              </div>

              <div>
                <label className="block text-sm text-gray-400 mb-1">Utilisateurs autorisés</label>
                <input
                  type="text"
                  placeholder="user1, user2, @group1"
                  value={newShare.validUsers}
                  onChange={e => setNewShare({ ...newShare, validUsers: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
                <p className="text-xs text-gray-500 mt-1">Séparés par des virgules. Préfixe @ pour les groupes</p>
              </div>

              <div>
                <label className="block text-sm text-gray-400 mb-1">Utilisateurs en écriture</label>
                <input
                  type="text"
                  placeholder="user1, @admins"
                  value={newShare.writeList}
                  onChange={e => setNewShare({ ...newShare, writeList: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
              </div>
            </div>

            <div className="flex justify-end gap-2 mt-6">
              <Button variant="secondary" onClick={() => setShowAddShareModal(false)}>
                Annuler
              </Button>
              <Button onClick={handleAddShare} loading={saving}>
                Créer
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Edit Share Modal */}
      {showEditShareModal && editingShare && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
          <div className="bg-gray-800 rounded-lg p-6 w-full max-w-lg border border-gray-700 max-h-[90vh] overflow-y-auto">
            <h2 className="text-xl font-bold mb-4 flex items-center gap-2">
              <Pencil className="w-5 h-5 text-blue-400" />
              Modifier "{editingShare.name}"
            </h2>

            <div className="space-y-4">
              <div>
                <label className="block text-sm text-gray-400 mb-1">Chemin local *</label>
                <input
                  type="text"
                  value={editShareForm.path}
                  onChange={e => setEditShareForm({ ...editShareForm, path: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
              </div>

              <div>
                <label className="block text-sm text-gray-400 mb-1">Description</label>
                <input
                  type="text"
                  value={editShareForm.comment}
                  onChange={e => setEditShareForm({ ...editShareForm, comment: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
              </div>

              <div className="grid grid-cols-3 gap-4">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={editShareForm.browseable}
                    onChange={e => setEditShareForm({ ...editShareForm, browseable: e.target.checked })}
                    className="rounded bg-gray-700 border-gray-600"
                  />
                  <span className="text-sm">Visible</span>
                </label>
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={editShareForm.writable}
                    onChange={e => setEditShareForm({ ...editShareForm, writable: e.target.checked })}
                    className="rounded bg-gray-700 border-gray-600"
                  />
                  <span className="text-sm">Ecriture</span>
                </label>
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={editShareForm.guestOk}
                    onChange={e => setEditShareForm({ ...editShareForm, guestOk: e.target.checked })}
                    className="rounded bg-gray-700 border-gray-600"
                  />
                  <span className="text-sm">Guest OK</span>
                </label>
              </div>

              <div>
                <label className="block text-sm text-gray-400 mb-1">Utilisateurs autorisés</label>
                <input
                  type="text"
                  placeholder="user1, user2, @group1"
                  value={editShareForm.validUsers}
                  onChange={e => setEditShareForm({ ...editShareForm, validUsers: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
              </div>

              <div>
                <label className="block text-sm text-gray-400 mb-1">Utilisateurs en écriture</label>
                <input
                  type="text"
                  placeholder="user1, @admins"
                  value={editShareForm.writeList}
                  onChange={e => setEditShareForm({ ...editShareForm, writeList: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
              </div>
            </div>

            <div className="flex justify-end gap-2 mt-6">
              <Button variant="secondary" onClick={() => { setShowEditShareModal(false); setEditingShare(null); }}>
                Annuler
              </Button>
              <Button onClick={handleEditShare} loading={saving}>
                Sauvegarder
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Add User Modal */}
      {showAddUserModal && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
          <div className="bg-gray-800 rounded-lg p-6 w-full max-w-md border border-gray-700">
            <h2 className="text-xl font-bold mb-4 flex items-center gap-2">
              <UserPlus className="w-5 h-5 text-blue-400" />
              Nouvel utilisateur Samba
            </h2>

            <div className="space-y-4">
              <div>
                <label className="block text-sm text-gray-400 mb-1">Nom d'utilisateur</label>
                <input
                  type="text"
                  placeholder="username"
                  value={newUser.username}
                  onChange={e => setNewUser({ ...newUser, username: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
                <p className="text-xs text-gray-500 mt-1">Doit exister en tant qu'utilisateur système</p>
              </div>

              <div>
                <label className="block text-sm text-gray-400 mb-1">Mot de passe</label>
                <div className="relative">
                  <input
                    type={showPassword ? 'text' : 'password'}
                    placeholder="••••••••"
                    value={newUser.password}
                    onChange={e => setNewUser({ ...newUser, password: e.target.value })}
                    className="w-full px-3 py-2 pr-10 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                  />
                  <button
                    type="button"
                    onClick={() => setShowPassword(!showPassword)}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-300"
                  >
                    {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
                  </button>
                </div>
              </div>

              <div>
                <label className="block text-sm text-gray-400 mb-1">Confirmer le mot de passe</label>
                <input
                  type={showPassword ? 'text' : 'password'}
                  placeholder="••••••••"
                  value={newUser.confirmPassword}
                  onChange={e => setNewUser({ ...newUser, confirmPassword: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
              </div>
            </div>

            <div className="flex justify-end gap-2 mt-6">
              <Button variant="secondary" onClick={() => { setShowAddUserModal(false); setNewUser({ username: '', password: '', confirmPassword: '' }); }}>
                Annuler
              </Button>
              <Button onClick={handleAddUser} loading={saving}>
                Créer
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Change Password Modal */}
      {showPasswordModal && editingUser && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
          <div className="bg-gray-800 rounded-lg p-6 w-full max-w-md border border-gray-700">
            <h2 className="text-xl font-bold mb-4 flex items-center gap-2">
              <Key className="w-5 h-5 text-yellow-400" />
              Mot de passe: {editingUser.username}
            </h2>

            <div className="space-y-4">
              <div>
                <label className="block text-sm text-gray-400 mb-1">Nouveau mot de passe</label>
                <div className="relative">
                  <input
                    type={showPassword ? 'text' : 'password'}
                    placeholder="••••••••"
                    value={newPassword.password}
                    onChange={e => setNewPassword({ ...newPassword, password: e.target.value })}
                    className="w-full px-3 py-2 pr-10 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                  />
                  <button
                    type="button"
                    onClick={() => setShowPassword(!showPassword)}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-300"
                  >
                    {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
                  </button>
                </div>
              </div>

              <div>
                <label className="block text-sm text-gray-400 mb-1">Confirmer</label>
                <input
                  type={showPassword ? 'text' : 'password'}
                  placeholder="••••••••"
                  value={newPassword.confirmPassword}
                  onChange={e => setNewPassword({ ...newPassword, confirmPassword: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
                />
              </div>
            </div>

            <div className="flex justify-end gap-2 mt-6">
              <Button variant="secondary" onClick={() => { setShowPasswordModal(false); setEditingUser(null); }}>
                Annuler
              </Button>
              <Button onClick={handleChangePassword} loading={saving}>
                Changer
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default Samba;
