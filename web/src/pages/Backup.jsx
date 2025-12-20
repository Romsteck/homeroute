import { useState, useEffect, useRef } from 'react';
import { HardDrive, Play, Settings, History, Plus, Trash2, CheckCircle, XCircle, Clock, Wifi, Square, AlertCircle } from 'lucide-react';
import { io } from 'socket.io-client';
import Card from '../components/Card';
import Button from '../components/Button';
import {
  getBackupConfig,
  saveBackupConfig,
  runBackup,
  getBackupHistory,
  testBackupConnection,
  cancelBackup,
  getBackupStatus
} from '../api/client';

function Backup() {
  const [config, setConfig] = useState(null);
  const [history, setHistory] = useState([]);
  const [sources, setSources] = useState([]);
  const [newSource, setNewSource] = useState('');
  const [loading, setLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [message, setMessage] = useState(null);
  const [progress, setProgress] = useState(null);
  const socketRef = useRef(null);

  // WebSocket connection
  useEffect(() => {
    const socket = io(window.location.origin);
    socketRef.current = socket;

    socket.on('backup:started', (data) => {
      setRunning(true);
      setProgress({
        status: 'started',
        sourcesCount: data.sourcesCount,
        sources: data.sources,
        currentSource: null,
        percent: 0,
        speed: null
      });
    });

    socket.on('backup:source-start', (data) => {
      setProgress(prev => ({
        ...prev,
        status: 'syncing',
        currentSource: data.sourceName,
        sourceIndex: data.sourceIndex,
        sourcesCount: data.sourcesCount,
        percent: 0,
        speed: null
      }));
    });

    socket.on('backup:progress', (data) => {
      setProgress(prev => ({
        ...prev,
        status: 'syncing',
        currentSource: data.sourceName,
        sourceIndex: data.sourceIndex,
        sourcesCount: data.sourcesCount,
        percent: data.percent,
        speed: data.speed,
        transferredBytes: data.transferredBytes
      }));
    });

    socket.on('backup:source-complete', (data) => {
      setProgress(prev => ({
        ...prev,
        status: 'source-complete',
        percent: 100
      }));
    });

    socket.on('backup:complete', (data) => {
      setRunning(false);
      setProgress(null);
      fetchData();
      if (data.cancelled) {
        setMessage({ type: 'warning', text: 'Backup annulé' });
      } else if (data.success) {
        setMessage({
          type: 'success',
          text: `Backup terminé: ${data.totalFiles} fichiers, ${formatSize(data.totalSize)}`
        });
      } else {
        setMessage({ type: 'error', text: 'Backup terminé avec des erreurs' });
      }
    });

    socket.on('backup:cancelled', () => {
      setRunning(false);
      setProgress(null);
      setCancelling(false);
      setMessage({ type: 'warning', text: 'Backup annulé' });
      fetchData();
    });

    socket.on('backup:error', (data) => {
      setRunning(false);
      setProgress(null);
      setMessage({ type: 'error', text: data.error });
      fetchData();
    });

    return () => {
      socket.disconnect();
    };
  }, []);

  // Initial data fetch + check if backup is running
  useEffect(() => {
    fetchData();
    checkBackupStatus();
  }, []);

  async function checkBackupStatus() {
    try {
      const res = await getBackupStatus();
      if (res.data.running) {
        setRunning(true);
        setProgress({ status: 'syncing', percent: 0 });
      }
    } catch (error) {
      console.error('Error checking backup status:', error);
    }
  }

  async function fetchData() {
    try {
      const [configRes, historyRes] = await Promise.all([
        getBackupConfig(),
        getBackupHistory()
      ]);

      if (configRes.data.success) {
        setConfig(configRes.data.config);
        setSources(configRes.data.config.sources || []);
      }
      if (historyRes.data.success) {
        setHistory(historyRes.data.history);
      }
    } catch (error) {
      console.error('Error:', error);
      setMessage({ type: 'error', text: 'Erreur de chargement' });
    } finally {
      setLoading(false);
    }
  }

  async function handleRunBackup() {
    setRunning(true);
    setMessage(null);
    setProgress({ status: 'starting', percent: 0 });
    try {
      // Fire and forget - progress comes via WebSocket
      runBackup().catch(error => {
        // Only handle network errors, backup errors come via WebSocket
        if (!error.response) {
          setMessage({ type: 'error', text: 'Erreur réseau' });
          setRunning(false);
          setProgress(null);
        }
      });
    } catch (error) {
      setMessage({ type: 'error', text: 'Erreur de lancement' });
      setRunning(false);
      setProgress(null);
    }
  }

  async function handleCancelBackup() {
    setCancelling(true);
    try {
      await cancelBackup();
      // WebSocket will handle the rest
    } catch (error) {
      setMessage({ type: 'error', text: 'Erreur d\'annulation' });
      setCancelling(false);
    }
  }

  async function handleTestConnection() {
    setTesting(true);
    setMessage(null);
    try {
      const res = await testBackupConnection();
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Connexion SMB OK' });
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: error.response?.data?.error || 'Test failed' });
    } finally {
      setTesting(false);
    }
  }

  async function handleSaveConfig() {
    setSaving(true);
    try {
      const res = await saveBackupConfig(sources);
      if (res.data.success) {
        setMessage({ type: 'success', text: 'Configuration sauvegardée' });
      } else {
        setMessage({ type: 'error', text: res.data.error });
      }
    } catch (error) {
      setMessage({ type: 'error', text: 'Erreur de sauvegarde' });
    } finally {
      setSaving(false);
    }
  }

  function handleAddSource() {
    if (!newSource.trim()) return;
    if (sources.includes(newSource.trim())) {
      setMessage({ type: 'error', text: 'Chemin déjà dans la liste' });
      return;
    }
    setSources([...sources, newSource.trim()]);
    setNewSource('');
  }

  function handleRemoveSource(path) {
    setSources(sources.filter(s => s !== path));
  }

  function formatSize(bytes) {
    if (!bytes) return '-';
    if (bytes < 1024) return bytes + ' B';
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
    if (bytes < 1024 * 1024 * 1024) return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
    return (bytes / (1024 * 1024 * 1024)).toFixed(2) + ' GB';
  }

  function formatDuration(ms) {
    if (!ms) return '-';
    if (ms < 1000) return ms + 'ms';
    if (ms < 60000) return (ms / 1000).toFixed(1) + 's';
    return (ms / 60000).toFixed(1) + ' min';
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-400"></div>
      </div>
    );
  }

  const smbConfigured = config?.smbServer && config?.smbShare;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Backup SMB</h1>
        <div className="flex gap-2">
          <Button onClick={handleTestConnection} loading={testing} variant="secondary" disabled={!smbConfigured}>
            <Wifi className="w-4 h-4" />
            Tester
          </Button>
          <Button
            onClick={handleRunBackup}
            loading={running}
            variant="success"
            disabled={!smbConfigured || sources.length === 0}
          >
            <Play className="w-4 h-4" />
            Lancer backup
          </Button>
        </div>
      </div>

      {message && (
        <div className={`p-4 rounded-lg flex items-center gap-2 ${
          message.type === 'success' ? 'bg-green-900/50 text-green-400' :
          message.type === 'warning' ? 'bg-yellow-900/50 text-yellow-400' :
          'bg-red-900/50 text-red-400'
        }`}>
          {message.type === 'success' ? <CheckCircle className="w-5 h-5" /> :
           message.type === 'warning' ? <AlertCircle className="w-5 h-5" /> :
           <XCircle className="w-5 h-5" />}
          {message.text}
        </div>
      )}

      {/* Progress bar when backup is running */}
      {running && progress && (
        <div className="bg-gray-800 border border-gray-700 rounded-lg p-4 space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <div className="animate-spin rounded-full h-4 w-4 border-2 border-blue-400 border-t-transparent"></div>
              <span className="font-medium">
                {progress.currentSource
                  ? `Backup: ${progress.currentSource} (${(progress.sourceIndex ?? 0) + 1}/${progress.sourcesCount || '?'})`
                  : 'Démarrage du backup...'}
              </span>
            </div>
            <div className="flex items-center gap-3">
              {progress.speed && (
                <span className="text-sm text-gray-400">{progress.speed}</span>
              )}
              <span className="text-sm font-mono text-blue-400">{progress.percent || 0}%</span>
            </div>
          </div>

          {/* Progress bar */}
          <div className="h-2 bg-gray-700 rounded-full overflow-hidden">
            <div
              className="h-full bg-blue-500 transition-all duration-150"
              style={{ width: `${progress.percent || 0}%` }}
            />
          </div>

          {/* Cancel button */}
          <div className="flex justify-end">
            <Button
              onClick={handleCancelBackup}
              loading={cancelling}
              variant="danger"
              className="text-sm"
            >
              <Square className="w-3 h-3" />
              Annuler
            </Button>
          </div>
        </div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card title="Configuration SMB" icon={HardDrive}>
          <div className="space-y-3 text-sm">
            <div className="flex justify-between">
              <span className="text-gray-400">Serveur</span>
              <span className="font-mono">{config?.smbServer || 'Non configuré'}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-gray-400">Partage</span>
              <span className="font-mono">{config?.smbShare || '-'}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-gray-400">Utilisateur</span>
              <span className="font-mono">{config?.smbUsername || '-'}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-gray-400">Mot de passe</span>
              <span className={config?.smbPasswordSet ? 'text-green-400' : 'text-yellow-400'}>
                {config?.smbPasswordSet ? 'Configuré' : 'Non configuré'}
              </span>
            </div>
            <div className="flex justify-between">
              <span className="text-gray-400">Point de montage</span>
              <span className="font-mono text-xs">{config?.mountPoint || '-'}</span>
            </div>
            <p className="text-xs text-gray-500 mt-4 pt-3 border-t border-gray-700">
              Configuration via fichier .env
            </p>
          </div>
        </Card>

        <Card
          title="Dossiers à sauvegarder"
          icon={Settings}
          actions={
            <Button onClick={handleSaveConfig} loading={saving} variant="primary" className="text-sm">
              Sauvegarder
            </Button>
          }
        >
          <div className="space-y-4">
            <div className="flex gap-2">
              <input
                type="text"
                placeholder="/chemin/vers/dossier"
                value={newSource}
                onChange={e => setNewSource(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && handleAddSource()}
                className="flex-1 px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm focus:outline-none focus:border-blue-500"
              />
              <Button onClick={handleAddSource}>
                <Plus className="w-4 h-4" />
              </Button>
            </div>

            <div className="space-y-2 max-h-48 overflow-y-auto">
              {sources.length === 0 ? (
                <p className="text-gray-500 text-sm text-center py-4">Aucun dossier configuré</p>
              ) : (
                sources.map(source => (
                  <div
                    key={source}
                    className="flex items-center justify-between bg-gray-900 rounded px-3 py-2"
                  >
                    <span className="font-mono text-sm truncate">{source}</span>
                    <button
                      onClick={() => handleRemoveSource(source)}
                      className="text-red-400 hover:text-red-300 ml-2"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                ))
              )}
            </div>

            <p className="text-xs text-gray-500">
              Utilise rsync --delete (miroir exact)
            </p>
          </div>
        </Card>
      </div>

      <Card title="Historique des backups" icon={History}>
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-gray-400 border-b border-gray-700">
                <th className="pb-2">Date</th>
                <th className="pb-2">Durée</th>
                <th className="pb-2">Fichiers</th>
                <th className="pb-2">Transféré</th>
                <th className="pb-2">Status</th>
              </tr>
            </thead>
            <tbody>
              {history.length === 0 ? (
                <tr>
                  <td colSpan={5} className="text-center py-4 text-gray-500">
                    Aucun backup effectué
                  </td>
                </tr>
              ) : (
                history.slice(0, 10).map((entry, i) => (
                  <tr key={i} className="border-b border-gray-700/50">
                    <td className="py-2">
                      <div className="flex items-center gap-2">
                        <Clock className="w-4 h-4 text-gray-500" />
                        {new Date(entry.timestamp).toLocaleString('fr-FR')}
                      </div>
                    </td>
                    <td className="py-2">{formatDuration(entry.duration)}</td>
                    <td className="py-2">{entry.filesTransferred ?? '-'}</td>
                    <td className="py-2">{formatSize(entry.transferredSize)}</td>
                    <td className="py-2">
                      {entry.status === 'success' ? (
                        <span className="text-green-400 flex items-center gap-1">
                          <CheckCircle className="w-4 h-4" /> OK
                        </span>
                      ) : entry.status === 'partial' ? (
                        <span className="text-yellow-400 flex items-center gap-1">
                          <AlertCircle className="w-4 h-4" /> Partiel
                        </span>
                      ) : entry.status === 'cancelled' ? (
                        <span className="text-gray-400 flex items-center gap-1">
                          <Square className="w-4 h-4" /> Annulé
                        </span>
                      ) : (
                        <span className="text-red-400 flex items-center gap-1" title={entry.error}>
                          <XCircle className="w-4 h-4" /> Erreur
                        </span>
                      )}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  );
}

export default Backup;
