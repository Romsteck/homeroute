import { useState, useEffect, useCallback } from 'react';
import {
  HardDrive, Play, RefreshCw, CheckCircle, XCircle,
  Clock, Database, Server, AlertTriangle, Loader2,
  Archive, Calendar
} from 'lucide-react';
import PageHeader from '../components/PageHeader';
import Button from '../components/Button';

// ── API helpers ──────────────────────────────────────────────────────────────

const api = (path, opts = {}) =>
  fetch(`/api${path}`, { credentials: 'include', ...opts }).then(r => r.json());

const getBackupStatus = () => api('/backup/status');
const getBackupRepos = () => api('/backup/repos');
const getBackupJobs = () => api('/backup/jobs');
const triggerBackup = () =>
  api('/backup/trigger', { method: 'POST' });

// ── Formatters ───────────────────────────────────────────────────────────────

const timeAgo = (dateStr) => {
  if (!dateStr) return '--';
  const diff = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
  if (diff < 60) return 'à l\'instant';
  if (diff < 3600) return `il y a ${Math.floor(diff / 60)}min`;
  if (diff < 86400) return `il y a ${Math.floor(diff / 3600)}h`;
  if (diff < 604800) return `il y a ${Math.floor(diff / 86400)}j`;
  return new Date(dateStr).toLocaleDateString('fr-FR');
};

const formatDuration = (secs) => {
  if (!secs && secs !== 0) return '--';
  if (secs < 60) return `${secs}s`;
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${m}m${s > 0 ? s + 's' : ''}`;
};

const formatDate = (dateStr) => {
  if (!dateStr) return '--';
  return new Date(dateStr).toLocaleString('fr-FR', {
    day: '2-digit', month: '2-digit', year: 'numeric',
    hour: '2-digit', minute: '2-digit'
  });
};

const STAGE_LABELS = {
  idle: 'En attente',
  waking_server: 'Réveil du serveur',
  waiting_for_server: 'Attente connexion',
  running_backup: 'Sauvegarde en cours',
  verifying: 'Vérification',
  putting_to_sleep: 'Mise en veille',
  done: 'Terminé',
  failed: 'Échec',
};

const REPO_ICONS = {
  homeroute: '🏠',
  containers: '📦',
  git: '🌿',
  pixel: '🌀',
};

// ── Sub-components ───────────────────────────────────────────────────────────

function StatusBadge({ success, running, stage }) {
  if (running) {
    return (
      <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-medium bg-blue-900/40 text-blue-300 border border-blue-700/50">
        <Loader2 className="w-3 h-3 animate-spin" />
        {STAGE_LABELS[stage] || stage || 'En cours'}
      </span>
    );
  }
  if (success === true) {
    return (
      <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-medium bg-green-900/40 text-green-300 border border-green-700/50">
        <CheckCircle className="w-3 h-3" />
        Succès
      </span>
    );
  }
  if (success === false) {
    return (
      <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-medium bg-red-900/40 text-red-300 border border-red-700/50">
        <XCircle className="w-3 h-3" />
        Échec
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-medium bg-gray-700/50 text-gray-400 border border-gray-600/50">
      <Clock className="w-3 h-3" />
      Jamais exécuté
    </span>
  );
}

function RepoCard({ repo }) {
  const icon = REPO_ICONS[repo.name] || '💾';
  return (
    <div className="bg-gray-800 border border-gray-700 rounded-lg p-4">
      <div className="flex items-start justify-between mb-3">
        <div className="flex items-center gap-2">
          <span className="text-2xl">{icon}</span>
          <div>
            <h3 className="font-semibold text-white capitalize">{repo.name}</h3>
            <p className="text-xs text-gray-400">/backup/{repo.name}/rustic</p>
          </div>
        </div>
        <StatusBadge success={repo.last_success} />
      </div>

      <div className="space-y-1.5 text-sm">
        <div className="flex justify-between text-gray-400">
          <span>Dernier backup</span>
          <span className="text-gray-200">{timeAgo(repo.last_backup_at)}</span>
        </div>
        {repo.last_duration_secs !== null && repo.last_duration_secs !== undefined && (
          <div className="flex justify-between text-gray-400">
            <span>Durée</span>
            <span className="text-gray-200">{formatDuration(repo.last_duration_secs)}</span>
          </div>
        )}
        {repo.last_snapshot_id && (
          <div className="flex justify-between text-gray-400">
            <span>Snapshot</span>
            <span className="text-gray-200 font-mono text-xs">{repo.last_snapshot_id.slice(0, 12)}...</span>
          </div>
        )}
        {repo.last_error && (
          <div className="mt-2 p-2 bg-red-900/20 border border-red-800/30 rounded text-xs text-red-300">
            {repo.last_error}
          </div>
        )}
      </div>
    </div>
  );
}

function JobRow({ job }) {
  return (
    <tr className="border-b border-gray-700/50 hover:bg-gray-700/20">
      <td className="px-4 py-3 text-sm">
        <div className="flex items-center gap-2">
          <span>{REPO_ICONS[job.repo_name] || '💾'}</span>
          <span className="text-gray-200 capitalize">{job.repo_name}</span>
        </div>
      </td>
      <td className="px-4 py-3 text-sm text-gray-300">{formatDate(job.started_at)}</td>
      <td className="px-4 py-3 text-sm text-gray-400">{formatDuration(job.duration_secs)}</td>
      <td className="px-4 py-3">
        {job.success ? (
          <span className="inline-flex items-center gap-1 text-xs text-green-400">
            <CheckCircle className="w-3.5 h-3.5" /> Succès
          </span>
        ) : (
          <span className="inline-flex items-center gap-1 text-xs text-red-400">
            <XCircle className="w-3.5 h-3.5" /> Échec
          </span>
        )}
      </td>
      <td className="px-4 py-3 text-xs text-gray-400 max-w-xs truncate">{job.message}</td>
    </tr>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

function Backup() {
  const [status, setStatus] = useState(null);
  const [repos, setRepos] = useState([]);
  const [jobs, setJobs] = useState([]);
  const [loading, setLoading] = useState(true);
  const [triggering, setTriggering] = useState(false);
  const [message, setMessage] = useState(null);

  const fetchAll = useCallback(async () => {
    try {
      const [statusRes, reposRes, jobsRes] = await Promise.all([
        getBackupStatus(),
        getBackupRepos(),
        getBackupJobs(),
      ]);

      if (statusRes && !statusRes.error) setStatus(statusRes);
      if (Array.isArray(reposRes)) setRepos(reposRes);
      else if (reposRes && Array.isArray(reposRes.data)) setRepos(reposRes.data);
      if (Array.isArray(jobsRes)) setJobs(jobsRes);
      else if (jobsRes && Array.isArray(jobsRes.data)) setJobs(jobsRes.data);
    } catch (err) {
      console.error('Backup fetch error:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  // Initial load
  useEffect(() => {
    fetchAll();
  }, [fetchAll]);

  // Polling — faster when running
  useEffect(() => {
    const interval = status?.running ? 3000 : 10000;
    const timer = setInterval(fetchAll, interval);
    return () => clearInterval(timer);
  }, [fetchAll, status?.running]);

  const handleTrigger = async () => {
    if (triggering || status?.running) return;
    setTriggering(true);
    setMessage(null);
    try {
      const res = await triggerBackup();
      if (res.error) {
        setMessage({ type: 'error', text: res.error });
      } else {
        setMessage({ type: 'success', text: res.message || 'Pipeline démarré' });
        setTimeout(fetchAll, 1000);
      }
    } catch (err) {
      setMessage({ type: 'error', text: 'Erreur réseau' });
    } finally {
      setTriggering(false);
    }
  };

  const isRunning = status?.running || false;
  const stage = status?.stage || 'idle';

  return (
    <div className="p-6 space-y-6 max-w-6xl">
      <PageHeader
        icon={Archive}
        title="Sauvegarde"
        subtitle="Gestion des sauvegardes Rustic — 4 repos — Rétention 7j/4sem/6mois"
      >
        <Button
          onClick={handleTrigger}
          disabled={isRunning || triggering}
          className="flex items-center gap-2"
        >
          {isRunning || triggering ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <Play className="w-4 h-4" />
          )}
          {isRunning ? 'En cours...' : 'Lancer un backup'}
        </Button>
        <Button variant="secondary" onClick={fetchAll} className="flex items-center gap-2">
          <RefreshCw className="w-4 h-4" />
          Actualiser
        </Button>
      </PageHeader>

      {/* Message */}
      {message && (
        <div className={`p-3 rounded border text-sm flex items-center gap-2 ${
          message.type === 'error'
            ? 'bg-red-900/20 border-red-700/50 text-red-300'
            : 'bg-green-900/20 border-green-700/50 text-green-300'
        }`}>
          {message.type === 'error' ? <AlertTriangle className="w-4 h-4 flex-shrink-0" /> : <CheckCircle className="w-4 h-4 flex-shrink-0" />}
          {message.text}
        </div>
      )}

      {/* Pipeline Status */}
      <div className="bg-gray-800 border border-gray-700 rounded-lg p-5">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold flex items-center gap-2">
            <Server className="w-5 h-5 text-blue-400" />
            État du pipeline
          </h2>
          <StatusBadge success={status?.last_run_success} running={isRunning} stage={stage} />
        </div>

        {loading ? (
          <div className="flex items-center gap-2 text-gray-400">
            <Loader2 className="w-4 h-4 animate-spin" />
            Chargement...
          </div>
        ) : (
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
            <div>
              <p className="text-gray-400 mb-1">Dernier run</p>
              <p className="text-gray-200">{timeAgo(status?.last_run_at)}</p>
            </div>
            <div>
              <p className="text-gray-400 mb-1">Durée</p>
              <p className="text-gray-200">{formatDuration(status?.last_run_duration_secs)}</p>
            </div>
            <div>
              <p className="text-gray-400 mb-1">Statut</p>
              <p className="text-gray-200">
                {isRunning
                  ? (status?.current_message || STAGE_LABELS[stage] || stage)
                  : (status?.last_run_success === true ? '✅ Succès' : status?.last_run_success === false ? '❌ Échec' : '—')}
              </p>
            </div>
            <div>
              <p className="text-gray-400 mb-1">Planifié</p>
              <p className="text-gray-200 flex items-center gap-1">
                <Calendar className="w-3.5 h-3.5" />
                03:00 UTC / jour
              </p>
            </div>
          </div>
        )}

        {isRunning && status?.current_message && (
          <div className="mt-4 p-3 bg-blue-900/20 border border-blue-700/30 rounded text-sm text-blue-300 flex items-center gap-2">
            <Loader2 className="w-3.5 h-3.5 animate-spin flex-shrink-0" />
            {status.current_message}
          </div>
        )}

        {!isRunning && status?.last_run_message && (
          <div className={`mt-4 p-3 rounded text-sm border ${
            status.last_run_success
              ? 'bg-green-900/10 border-green-700/30 text-green-300'
              : 'bg-red-900/10 border-red-700/30 text-red-300'
          }`}>
            {status.last_run_message}
          </div>
        )}
      </div>

      {/* Per-repo status */}
      <div>
        <h2 className="text-lg font-semibold mb-3 flex items-center gap-2">
          <Database className="w-5 h-5 text-purple-400" />
          Repos ({repos.length})
        </h2>
        {loading ? (
          <div className="flex items-center gap-2 text-gray-400 text-sm">
            <Loader2 className="w-4 h-4 animate-spin" />
            Chargement...
          </div>
        ) : repos.length === 0 ? (
          <div className="text-gray-400 text-sm">Aucune donnée de repo disponible.</div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
            {repos.map((repo) => (
              <RepoCard key={repo.name} repo={repo} />
            ))}
          </div>
        )}
      </div>

      {/* Job history */}
      <div>
        <h2 className="text-lg font-semibold mb-3 flex items-center gap-2">
          <Clock className="w-5 h-5 text-yellow-400" />
          Historique ({jobs.length})
        </h2>
        {loading ? (
          <div className="flex items-center gap-2 text-gray-400 text-sm">
            <Loader2 className="w-4 h-4 animate-spin" />
            Chargement...
          </div>
        ) : jobs.length === 0 ? (
          <div className="bg-gray-800 border border-gray-700 rounded-lg p-6 text-center text-gray-400">
            <Clock className="w-8 h-8 mx-auto mb-2 opacity-40" />
            <p className="text-sm">Aucun job dans l'historique.</p>
            <p className="text-xs mt-1">Lancez un backup pour commencer.</p>
          </div>
        ) : (
          <div className="bg-gray-800 border border-gray-700 rounded-lg overflow-hidden">
            <table className="w-full text-sm">
              <thead className="border-b border-gray-700 bg-gray-800/80">
                <tr>
                  <th className="px-4 py-3 text-left text-gray-400 font-medium">Repo</th>
                  <th className="px-4 py-3 text-left text-gray-400 font-medium">Date</th>
                  <th className="px-4 py-3 text-left text-gray-400 font-medium">Durée</th>
                  <th className="px-4 py-3 text-left text-gray-400 font-medium">Statut</th>
                  <th className="px-4 py-3 text-left text-gray-400 font-medium">Message</th>
                </tr>
              </thead>
              <tbody>
                {jobs.map((job) => (
                  <JobRow key={job.id} job={job} />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}

export default Backup;
