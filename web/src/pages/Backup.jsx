import { useEffect, useMemo, useState } from 'react';
import { Archive, CheckCircle2, Clock3, Loader2, Play, XCircle } from 'lucide-react';
import PageHeader from '../components/PageHeader';
import Button from '../components/Button';
import useWebSocket from '../hooks/useWebSocket';

const api = (path, opts = {}) =>
  fetch(`/api${path}`, { credentials: 'include', ...opts }).then((r) => r.json());

const getBackupStatus = () => api('/backup/status');
const getBackupRepos = () => api('/backup/repos');
const getBackupJobs = () => api('/backup/jobs');
const getBackupProgress = () => api('/backup/progress');
const triggerBackup = () => api('/backup/trigger', { method: 'POST' });

const REPO_LABELS = {
  homeroute: 'HomeRoute',
  containers: 'Containers',
  git: 'Git',
  pixel: 'Pixel',
};

const timeAgo = (dateStr) => {
  if (!dateStr) return 'Jamais';
  const diff = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
  if (diff < 60) return 'à l’instant';
  if (diff < 3600) return `il y a ${Math.floor(diff / 60)} min`;
  if (diff < 86400) return `il y a ${Math.floor(diff / 3600)} h`;
  if (diff < 604800) return `il y a ${Math.floor(diff / 86400)} j`;
  return new Date(dateStr).toLocaleDateString('fr-FR');
};

const formatDate = (dateStr) =>
  dateStr
    ? new Date(dateStr).toLocaleString('fr-FR', {
        day: '2-digit',
        month: '2-digit',
        year: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
      })
    : '—';

const formatDuration = (secs) => {
  if (secs === null || secs === undefined) return '—';
  if (secs < 60) return `${secs}s`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m${s ? ` ${s}s` : ''}`;
};

const freshnessClasses = (dateStr) => {
  if (!dateStr) return 'border-red-500/30 bg-red-500/10 text-red-200';
  const ageHours = (Date.now() - new Date(dateStr).getTime()) / 3600000;
  if (ageHours < 24) return 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200';
  if (ageHours < 24 * 7) return 'border-amber-500/30 bg-amber-500/10 text-amber-200';
  return 'border-red-500/30 bg-red-500/10 text-red-200';
};

const statusBadge = (job) => {
  if (!job) return { label: '—', className: 'border-gray-700 bg-gray-800 text-gray-300', icon: Clock3 };
  if (job.success) return { label: 'Succès', className: 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200', icon: CheckCircle2 };
  return { label: 'Échec', className: 'border-red-500/30 bg-red-500/10 text-red-200', icon: XCircle };
};

function RepoRow({ repo, active }) {
  return (
    <div className={`flex items-center justify-between gap-4 rounded-2xl border px-4 py-4 ${active ? 'border-blue-500/40 bg-blue-500/10' : 'border-gray-700/70 bg-gray-800/70'}`}>
      <div className="min-w-0">
        <div className="truncate text-sm font-semibold text-white">{REPO_LABELS[repo.name] || repo.name}</div>
        <div className="text-sm text-gray-400">Dernière sauvegarde {timeAgo(repo.last_backup_at)}</div>
      </div>
      <span className={`inline-flex shrink-0 items-center rounded-full border px-2.5 py-1 text-xs font-medium ${freshnessClasses(repo.last_backup_at)}`}>
        {repo.last_backup_at ? timeAgo(repo.last_backup_at) : 'Jamais'}
      </span>
    </div>
  );
}

export default function Backup() {
  const [status, setStatus] = useState(null);
  const [repos, setRepos] = useState([]);
  const [latestJob, setLatestJob] = useState(null);
  const [progress, setProgress] = useState({ running: false });
  const [loading, setLoading] = useState(true);
  const [triggering, setTriggering] = useState(false);
  const [message, setMessage] = useState(null);

  const fetchInitial = async () => {
    try {
      const [statusRes, reposRes, jobsRes, progressRes] = await Promise.all([
        getBackupStatus(),
        getBackupRepos(),
        getBackupJobs(),
        getBackupProgress(),
      ]);
      setStatus(statusRes && !statusRes.error ? statusRes : null);
      setRepos(Array.isArray(reposRes) ? reposRes : reposRes?.data || []);
      const jobs = Array.isArray(jobsRes) ? jobsRes : jobsRes?.data || [];
      setLatestJob(jobs[0] || null);
      setProgress(progressRes && !progressRes.error ? progressRes : { running: false });
    } catch (error) {
      console.error('Backup fetch error:', error);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchInitial();
  }, []);

  useWebSocket({
    'backup:live': (data) => {
      if (data?.status) setStatus(data.status);
      if (data?.progress) setProgress(data.progress);
      if (Array.isArray(data?.repos)) setRepos(data.repos);
      setLatestJob(data?.latestJob || null);
      setLoading(false);
    },
  });

  const handleTrigger = async () => {
    if (triggering || status?.running || progress?.running) return;
    setTriggering(true);
    setMessage(null);
    try {
      const res = await triggerBackup();
      if (res?.error) {
        setMessage({ type: 'error', text: res.error });
      } else {
        setMessage({ type: 'success', text: res?.message || 'Sauvegarde lancée' });
      }
    } catch {
      setMessage({ type: 'error', text: 'Erreur réseau' });
    } finally {
      setTriggering(false);
    }
  };

  const isRunning = Boolean(status?.running || progress?.running);
  const progressPct = Math.max(0, Math.min(100, Number(progress?.progress || 0)));
  const currentRepo = progress?.current_repo || 'backup';
  const currentPhase = progress?.phase ? ` (${progress.phase})` : '';
  const lastRun = useMemo(() => latestJob || null, [latestJob]);
  const lastRunBadge = statusBadge(lastRun);
  const StatusIcon = lastRunBadge.icon;

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-6 p-6">
      <PageHeader
        icon={Archive}
        title="Sauvegarde"
        subtitle="Une vue simple, en temps réel."
      />

      {message && (
        <div className={`rounded-2xl border px-4 py-3 text-sm ${message.type === 'error' ? 'border-red-500/30 bg-red-500/10 text-red-200' : 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200'}`}>
          {message.text}
        </div>
      )}

      <section className="rounded-3xl border border-gray-700/70 bg-gray-800/70 p-6">
        <div className="flex flex-col gap-5">
          <div>
            <div className="text-sm font-medium text-gray-400">Action</div>
            <div className="mt-1 text-2xl font-semibold text-white">
              {isRunning ? 'Sauvegarde en cours' : 'Prêt à lancer une sauvegarde'}
            </div>
          </div>

          <Button
            onClick={handleTrigger}
            disabled={isRunning || triggering || loading}
            className="flex h-14 items-center justify-center gap-3 rounded-2xl text-base font-semibold"
          >
            {isRunning || triggering ? <Loader2 className="h-5 w-5 animate-spin" /> : <Play className="h-5 w-5" />}
            {isRunning ? 'Sauvegarde en cours…' : 'Lancer une sauvegarde'}
          </Button>

          {isRunning && (
            <div className="rounded-2xl border border-blue-500/30 bg-blue-500/10 p-5">
              <div className="mb-3 flex flex-wrap items-center justify-between gap-3 text-sm">
                <div className="font-medium text-blue-50">
                  Sauvegarde en cours — {currentRepo}{currentPhase} — {Math.round(progressPct)}%
                </div>
                <div className="text-blue-200">{Math.round(progressPct)}%</div>
              </div>
              <div className="h-4 overflow-hidden rounded-full bg-gray-950/60">
                <div
                  className="h-full rounded-full bg-gradient-to-r from-blue-500 to-cyan-400 transition-all duration-500"
                  style={{ width: `${progressPct}%` }}
                />
              </div>
              <div className="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-sm text-blue-100/80">
                <span>Vitesse : {progress?.speed || '—'}</span>
                <span>Temps restant : {formatDuration(progress?.remaining_secs)}</span>
              </div>
            </div>
          )}
        </div>
      </section>

      {!isRunning && (
        <section className="space-y-3">
          <div>
            <h2 className="text-lg font-semibold text-white">Repos</h2>
            <p className="text-sm text-gray-400">Un coup d’œil suffit pour voir si tout est frais.</p>
          </div>
          <div className="grid gap-3">
            {repos.map((repo) => (
              <RepoRow key={repo.name} repo={repo} active={false} />
            ))}
            {!loading && repos.length === 0 && (
              <div className="rounded-2xl border border-dashed border-gray-700 bg-gray-800/50 px-4 py-6 text-sm text-gray-400">
                Aucun repo remonté par l’API.
              </div>
            )}
          </div>
        </section>
      )}

      <section className="rounded-3xl border border-gray-700/70 bg-gray-800/70 p-5">
        <div className="mb-3 text-sm font-medium text-gray-400">Dernière sauvegarde</div>
        {lastRun ? (
          <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <div className="min-w-0">
              <div className="truncate text-base font-semibold text-white">
                {REPO_LABELS[lastRun.repo_name] || lastRun.repo_name}
              </div>
              <div className="text-sm text-gray-400">
                {formatDate(lastRun.started_at)} · {formatDuration(lastRun.duration_secs)}
              </div>
            </div>
            <div className="flex items-center gap-3">
              <span className={`inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-sm ${lastRunBadge.className}`}>
                <StatusIcon className="h-4 w-4" />
                {lastRunBadge.label}
              </span>
            </div>
          </div>
        ) : (
          <div className="text-sm text-gray-400">Aucune sauvegarde enregistrée pour le moment.</div>
        )}
      </section>
    </div>
  );
}
