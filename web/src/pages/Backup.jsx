import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  Archive, Play, RefreshCw, CheckCircle, XCircle, Clock,
  Loader2, Database, Gauge, FolderSync, TimerReset, HardDrive,
  Server, ShieldAlert
} from 'lucide-react';
import PageHeader from '../components/PageHeader';
import Button from '../components/Button';

const api = (path, opts = {}) => fetch(`/api${path}`, { credentials: 'include', ...opts }).then(r => r.json());
const getBackupStatus = () => api('/backup/status');
const getBackupRepos = () => api('/backup/repos');
const getBackupJobs = () => api('/backup/jobs');
const getBackupProgress = () => api('/backup/progress');
const triggerBackup = () => api('/backup/trigger', { method: 'POST' });

const REPO_META = {
  homeroute: { icon: '🏠', label: 'HomeRoute' },
  containers: { icon: '📦', label: 'Containers' },
  git: { icon: '🌿', label: 'Git' },
  pixel: { icon: '🌀', label: 'Pixel' },
};

const STAGE_LABELS = {
  idle: 'Inactif', waking_server: 'Réveil', waiting_for_server: 'Connexion', running_backup: 'Sauvegarde', verifying: 'Vérification', putting_to_sleep: 'Mise en veille', done: 'Terminé', failed: 'Échec',
};
const PHASE_LABELS = { idle: 'Préparation', rsync: 'rsync', rustic: 'rustic', forget: 'rétention', sleep: 'veille', done: 'terminé', failed: 'échec' };

const timeAgo = (dateStr) => {
  if (!dateStr) return 'Jamais';
  const diff = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
  if (diff < 60) return 'à l’instant';
  if (diff < 3600) return `il y a ${Math.floor(diff / 60)} min`;
  if (diff < 86400) return `il y a ${Math.floor(diff / 3600)} h`;
  if (diff < 604800) return `il y a ${Math.floor(diff / 86400)} j`;
  return new Date(dateStr).toLocaleDateString('fr-FR');
};
const formatDate = (dateStr) => dateStr ? new Date(dateStr).toLocaleString('fr-FR', { day: '2-digit', month: '2-digit', year: 'numeric', hour: '2-digit', minute: '2-digit' }) : '—';
const formatDuration = (secs) => {
  if (secs === null || secs === undefined) return '—';
  if (secs < 60) return `${secs}s`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m${s ? ` ${s}s` : ''}`;
};
const formatBytes = (bytes) => {
  if (bytes === null || bytes === undefined) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes; let unit = 0;
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit += 1; }
  return `${value >= 10 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
};
const freshnessTone = (dateStr) => {
  if (!dateStr) return 'bg-gray-700/60 text-gray-300 border-gray-600/60';
  const hours = (Date.now() - new Date(dateStr).getTime()) / 3600000;
  if (hours < 24) return 'bg-emerald-500/15 text-emerald-300 border-emerald-500/30';
  if (hours < 24 * 7) return 'bg-amber-500/15 text-amber-300 border-amber-500/30';
  return 'bg-red-500/15 text-red-300 border-red-500/30';
};

function Metric({ icon: Icon, label, value }) {
  return (
    <div className="rounded-xl border border-gray-700/70 bg-gray-900/60 p-3">
      <div className="mb-1 flex items-center gap-2 text-xs uppercase tracking-wide text-gray-500"><Icon className="h-3.5 w-3.5" />{label}</div>
      <div className="text-sm font-medium text-gray-100">{value}</div>
    </div>
  );
}

function RepoCard({ repo, isActive }) {
  const meta = REPO_META[repo.name] || { icon: '💾', label: repo.name };
  return (
    <div className={`rounded-2xl border p-4 transition ${isActive ? 'border-blue-500/50 bg-blue-500/10 shadow-[0_0_0_1px_rgba(59,130,246,0.15)]' : 'border-gray-700/70 bg-gray-800/70'}`}>
      <div className="mb-4 flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl bg-gray-900 text-2xl">{meta.icon}</div>
          <div className="min-w-0">
            <div className="truncate text-base font-semibold text-white">{meta.label}</div>
            <div className="text-xs text-gray-400">/backup/{repo.name}/rustic</div>
          </div>
        </div>
        <span className={`inline-flex shrink-0 items-center rounded-full border px-2.5 py-1 text-xs font-medium ${freshnessTone(repo.last_backup_at)}`}>{timeAgo(repo.last_backup_at)}</span>
      </div>
      <div className="grid grid-cols-2 gap-3 text-sm">
        <Metric icon={Clock} label="Dernier backup" value={formatDate(repo.last_backup_at)} />
        <Metric icon={TimerReset} label="Durée" value={formatDuration(repo.last_duration_secs)} />
        <Metric icon={HardDrive} label="Dernier transfert" value={formatBytes(repo.last_transferred_bytes)} />
        <Metric icon={Database} label="Snapshot" value={repo.last_snapshot_id ? `${repo.last_snapshot_id.slice(0, 12)}…` : '—'} />
      </div>
      <div className="mt-4 flex items-center gap-2 text-sm">
        {repo.last_success === true && <span className="inline-flex items-center gap-1 rounded-full border border-emerald-500/30 bg-emerald-500/15 px-2.5 py-1 text-emerald-300"><CheckCircle className="h-4 w-4" /> OK</span>}
        {repo.last_success === false && <span className="inline-flex items-center gap-1 rounded-full border border-red-500/30 bg-red-500/15 px-2.5 py-1 text-red-300"><XCircle className="h-4 w-4" /> Échec</span>}
        {repo.last_success === null && <span className="inline-flex items-center gap-1 rounded-full border border-gray-600/70 bg-gray-700/50 px-2.5 py-1 text-gray-300"><Clock className="h-4 w-4" /> Jamais exécuté</span>}
      </div>
      {repo.last_error && <div className="mt-3 rounded-xl border border-red-500/20 bg-red-500/10 p-3 text-xs text-red-200">{repo.last_error}</div>}
    </div>
  );
}

export default function Backup() {
  const [status, setStatus] = useState(null);
  const [repos, setRepos] = useState([]);
  const [jobs, setJobs] = useState([]);
  const [progress, setProgress] = useState({ running: false });
  const [loading, setLoading] = useState(true);
  const [triggering, setTriggering] = useState(false);
  const [message, setMessage] = useState(null);

  const fetchAll = useCallback(async () => {
    try {
      const [statusRes, reposRes, jobsRes, progressRes] = await Promise.all([getBackupStatus(), getBackupRepos(), getBackupJobs(), getBackupProgress()]);
      if (statusRes && !statusRes.error) setStatus(statusRes);
      setRepos(Array.isArray(reposRes) ? reposRes : reposRes?.data || []);
      const nextJobs = Array.isArray(jobsRes) ? jobsRes : jobsRes?.data || [];
      setJobs(nextJobs.slice(0, 20));
      if (progressRes && !progressRes.error) setProgress(progressRes);
    } catch (err) {
      console.error('Backup fetch error:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchAll(); }, [fetchAll]);
  useEffect(() => {
    const interval = status?.running || progress?.running ? 2500 : 10000;
    const timer = setInterval(fetchAll, interval);
    return () => clearInterval(timer);
  }, [fetchAll, status?.running, progress?.running]);

  const handleTrigger = async () => {
    if (triggering || status?.running) return;
    setTriggering(true); setMessage(null);
    try {
      const res = await triggerBackup();
      if (res.error) setMessage({ type: 'error', text: res.error });
      else { setMessage({ type: 'success', text: res.message || 'Pipeline démarré' }); setTimeout(fetchAll, 800); }
    } catch {
      setMessage({ type: 'error', text: 'Erreur réseau' });
    } finally { setTriggering(false); }
  };

  const isRunning = !!(status?.running || progress?.running);
  const activeRepo = progress?.current_repo;
  const progressPct = Math.max(0, Math.min(100, Number(progress?.progress || 0)));
  const overview = useMemo(() => ({
    totalRepos: repos.length,
    successCount: repos.filter(r => r.last_success === true).length,
    staleCount: repos.filter(r => !r.last_backup_at || (Date.now() - new Date(r.last_backup_at).getTime()) / 3600000 > 24 * 7).length,
  }), [repos]);

  return (
    <div className="max-w-7xl space-y-6 p-6">
      <PageHeader icon={Archive} title="Sauvegarde" subtitle="Rustic + rsync — progression live, fraîcheur des repos et historique des jobs">
        <div className="flex flex-wrap items-center gap-3">
          <span className={`inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-sm font-medium ${isRunning ? 'border-blue-500/40 bg-blue-500/15 text-blue-200' : 'border-gray-600 bg-gray-800 text-gray-300'}`}>
            {isRunning ? <Loader2 className="h-4 w-4 animate-spin" /> : <Archive className="h-4 w-4" />}
            {isRunning ? 'En cours' : 'Inactif'}
          </span>
          <Button onClick={handleTrigger} disabled={isRunning || triggering} className="flex items-center gap-2">{isRunning || triggering ? <Loader2 className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}Lancer</Button>
          <Button variant="secondary" onClick={fetchAll} className="flex items-center gap-2"><RefreshCw className="h-4 w-4" />Actualiser</Button>
        </div>
      </PageHeader>

      {message && <div className={`rounded-2xl border p-4 text-sm ${message.type === 'error' ? 'border-red-500/30 bg-red-500/10 text-red-200' : 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200'}`}>{message.text}</div>}

      <div className="grid gap-4 md:grid-cols-3">
        <Metric icon={Database} label="Repos suivis" value={overview.totalRepos || '—'} />
        <Metric icon={CheckCircle} label="Repos OK" value={overview.successCount} />
        <Metric icon={ShieldAlert} label="Repos à surveiller" value={overview.staleCount} />
      </div>

      <section className="rounded-3xl border border-gray-700/70 bg-gray-800/70 p-5">
        <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="text-lg font-semibold text-white">Progression active</h2>
            <p className="text-sm text-gray-400">{isRunning ? (status?.current_message || progress?.detail || 'Pipeline en cours…') : 'Aucun backup en cours.'}</p>
          </div>
          <div className="rounded-full border border-gray-700 bg-gray-900/70 px-3 py-1 text-sm text-gray-300">{STAGE_LABELS[status?.stage] || 'Inactif'} · {PHASE_LABELS[progress?.phase] || '—'}</div>
        </div>
        <div className="mb-3 h-3 overflow-hidden rounded-full bg-gray-900"><div className="h-full rounded-full bg-gradient-to-r from-blue-500 via-cyan-400 to-emerald-400 transition-all duration-500" style={{ width: `${progressPct}%` }} /></div>
        <div className="mb-5 flex flex-wrap items-center justify-between gap-2 text-sm"><div className="text-gray-200">{activeRepo ? <>Repo actif: <span className="font-semibold capitalize">{activeRepo}</span></> : 'Aucun repo actif'}</div><div className="font-medium text-white">{progressPct.toFixed(1)}%</div></div>
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          <Metric icon={FolderSync} label="Phase" value={PHASE_LABELS[progress?.phase] || '—'} />
          <Metric icon={Gauge} label="Vitesse" value={progress?.speed || '—'} />
          <Metric icon={Clock} label="Temps écoulé" value={formatDuration(progress?.elapsed_secs)} />
          <Metric icon={TimerReset} label="Temps restant" value={progress?.remaining_secs !== null && progress?.remaining_secs !== undefined ? formatDuration(progress?.remaining_secs) : '—'} />
          <Metric icon={Database} label="Fichiers" value={progress?.total_files ? `${progress?.files_processed || 0} / ${progress.total_files}` : (progress?.files_processed ?? '—')} />
          <Metric icon={HardDrive} label="Transféré" value={formatBytes(progress?.bytes_transferred)} />
          <Metric icon={Server} label="Dernier run" value={timeAgo(status?.last_run_at)} />
          <Metric icon={Archive} label="Résultat" value={status?.last_run_success === true ? 'Succès' : status?.last_run_success === false ? 'Échec' : '—'} />
        </div>
      </section>

      <section>
        <div className="mb-4 flex items-center justify-between"><h2 className="text-lg font-semibold text-white">Repos</h2><span className="text-sm text-gray-400">Grille synthétique</span></div>
        <div className="grid gap-4 lg:grid-cols-2">
          {repos.map(repo => <RepoCard key={repo.name} repo={repo} isActive={activeRepo === repo.name} />)}
          {!loading && repos.length === 0 && <div className="rounded-2xl border border-dashed border-gray-700 bg-gray-800/50 p-6 text-sm text-gray-400">Aucun repo remonté par l’API.</div>}
        </div>
      </section>

      <section className="rounded-3xl border border-gray-700/70 bg-gray-800/70 p-5">
        <div className="mb-4"><h2 className="text-lg font-semibold text-white">Historique</h2><p className="text-sm text-gray-400">20 derniers jobs maximum.</p></div>
        <div className="max-h-[28rem] overflow-auto rounded-2xl border border-gray-700/70">
          <table className="min-w-full text-sm">
            <thead className="sticky top-0 bg-gray-900/95 text-left text-gray-400"><tr><th className="px-4 py-3 font-medium">Repo</th><th className="px-4 py-3 font-medium">Démarré</th><th className="px-4 py-3 font-medium">Durée</th><th className="px-4 py-3 font-medium">Statut</th><th className="px-4 py-3 font-medium">Détail</th></tr></thead>
            <tbody>
              {jobs.map(job => (
                <tr key={job.id} className="border-t border-gray-700/60 align-top hover:bg-gray-700/20">
                  <td className="px-4 py-3 text-gray-200"><div className="flex items-center gap-2"><span>{REPO_META[job.repo_name]?.icon || '💾'}</span><span className="capitalize">{job.repo_name}</span></div></td>
                  <td className="px-4 py-3 text-gray-300">{formatDate(job.started_at)}</td>
                  <td className="px-4 py-3 text-gray-300">{formatDuration(job.duration_secs)}</td>
                  <td className="px-4 py-3">{job.success ? <span className="inline-flex items-center gap-1 rounded-full border border-emerald-500/30 bg-emerald-500/10 px-2.5 py-1 text-xs text-emerald-300"><CheckCircle className="h-3.5 w-3.5" />Succès</span> : <span className="inline-flex items-center gap-1 rounded-full border border-red-500/30 bg-red-500/10 px-2.5 py-1 text-xs text-red-300"><XCircle className="h-3.5 w-3.5" />Échec</span>}</td>
                  <td className="px-4 py-3 text-gray-400"><div className="max-w-xl whitespace-normal break-words">{job.message || '—'}</div></td>
                </tr>
              ))}
              {!loading && jobs.length === 0 && <tr><td colSpan="5" className="px-4 py-8 text-center text-gray-400">Aucun job enregistré.</td></tr>}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
