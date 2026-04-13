import { useState, useEffect, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import useWebSocket from '../hooks/useWebSocket';
import {
  Boxes,
  Plus,
  Play,
  Square,
  RefreshCw,
  Trash2,
  X,
  ExternalLink,
  ScrollText,
  Loader2,
  Globe,
  Lock,
} from 'lucide-react';
import PageHeader from '../components/PageHeader';
import {
  listApps,
  createApp,
  controlApp,
  deleteApp,
} from '../api/client';

const STACKS = [
  { value: 'next-js', label: 'Next.js' },
  { value: 'axum-vite', label: 'Vite+Rust' },
  { value: 'axum', label: 'Rust Only' },
];

const SLUG_RE = /^[a-z][a-z0-9-]*$/;

function slugify(name) {
  return name
    .toLowerCase()
    .replace(/\s+/g, '-')
    .replace(/[^a-z0-9-]/g, '')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '');
}

function statusBadge(state) {
  const s = (state || '').toLowerCase();
  if (s === 'running') {
    return <span className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-green-500/15 text-green-400 border border-green-500/20">Running</span>;
  }
  if (s === 'crashed' || s === 'failed' || s === 'error') {
    return <span className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-red-500/15 text-red-400 border border-red-500/20">Crashed</span>;
  }
  if (s === 'starting') {
    return <span className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-yellow-500/15 text-yellow-400 border border-yellow-500/20">Starting</span>;
  }
  return <span className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-gray-600/30 text-gray-300 border border-gray-600">Stopped</span>;
}

function visibilityBadge(visibility) {
  const isPublic = visibility === 'public';
  return (
    <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs font-medium border ${
      isPublic
        ? 'bg-blue-500/10 text-blue-300 border-blue-500/20'
        : 'bg-gray-700 text-gray-300 border-gray-600'
    }`}>
      {isPublic ? <Globe className="w-3 h-3" /> : <Lock className="w-3 h-3" />}
      {isPublic ? 'Public' : 'Privé'}
    </span>
  );
}

function CreateAppModal({ onClose, onCreated }) {
  const [name, setName] = useState('');
  const [slug, setSlug] = useState('');
  const [slugManual, setSlugManual] = useState(false);
  const [stack, setStack] = useState('axum-vite');
  const [visibility, setVisibility] = useState('private');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState(null);

  function handleNameChange(val) {
    setName(val);
    if (!slugManual) setSlug(slugify(val));
  }

  function handleSlugChange(val) {
    setSlugManual(true);
    setSlug(slugify(val));
  }

  async function handleSubmit(e) {
    e.preventDefault();
    if (!name.trim()) {
      setError('Le nom est requis');
      return;
    }
    if (!SLUG_RE.test(slug)) {
      setError('Slug invalide (lettres minuscules, chiffres, tirets — doit commencer par une lettre)');
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await createApp({
        name: name.trim(),
        slug,
        stack,
        visibility,
      });
      onCreated();
    } catch (err) {
      setError(err.response?.data?.error || err.message || 'Création échouée');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4" onClick={onClose}>
      <div
        className="w-full max-w-md bg-gray-800 border border-gray-700 rounded-lg shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-gray-700">
          <h2 className="text-lg font-semibold text-white">Nouvelle application</h2>
          <button onClick={onClose} className="text-gray-400 hover:text-white">
            <X className="w-5 h-5" />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="p-5 space-y-4">
          {error && (
            <div className="bg-red-500/10 border border-red-500/30 rounded-md px-3 py-2">
              <p className="text-sm text-red-400">{error}</p>
            </div>
          )}

          <div>
            <label className="block text-xs font-medium text-gray-400 mb-1">Nom</label>
            <input
              type="text"
              value={name}
              onChange={(e) => handleNameChange(e.target.value)}
              placeholder="Mon application"
              autoFocus
              className="w-full px-3 py-2 text-sm bg-gray-900 border border-gray-700 text-white rounded-md focus:outline-none focus:border-blue-500"
            />
          </div>

          <div>
            <label className="block text-xs font-medium text-gray-400 mb-1">Slug</label>
            <input
              type="text"
              value={slug}
              onChange={(e) => handleSlugChange(e.target.value)}
              placeholder="mon-application"
              className="w-full px-3 py-2 text-sm bg-gray-900 border border-gray-700 text-white font-mono rounded-md focus:outline-none focus:border-blue-500"
            />
          </div>

          <div>
            <label className="block text-xs font-medium text-gray-400 mb-1">Stack</label>
            <select
              value={stack}
              onChange={(e) => setStack(e.target.value)}
              className="w-full px-3 py-2 text-sm bg-gray-900 border border-gray-700 text-white rounded-md focus:outline-none focus:border-blue-500"
            >
              {STACKS.map((s) => (
                <option key={s.value} value={s.value}>{s.label}</option>
              ))}
            </select>
          </div>

          <div>
            <label className="block text-xs font-medium text-gray-400 mb-2">Visibilité</label>
            <div className="flex gap-3">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="radio"
                  name="visibility"
                  value="private"
                  checked={visibility === 'private'}
                  onChange={() => setVisibility('private')}
                  className="text-blue-500 focus:ring-blue-500"
                />
                <span className="text-sm text-gray-300">Privée</span>
              </label>
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="radio"
                  name="visibility"
                  value="public"
                  checked={visibility === 'public'}
                  onChange={() => setVisibility('public')}
                  className="text-blue-500 focus:ring-blue-500"
                />
                <span className="text-sm text-gray-300">Publique</span>
              </label>
            </div>
          </div>

          <div className="flex justify-end gap-2 pt-3 border-t border-gray-700">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 text-sm text-gray-300 bg-gray-700 hover:bg-gray-600 rounded-md"
            >
              Annuler
            </button>
            <button
              type="submit"
              disabled={submitting}
              className="px-4 py-2 text-sm text-white bg-blue-500 hover:bg-blue-600 disabled:opacity-50 rounded-md flex items-center gap-2"
            >
              {submitting && <Loader2 className="w-4 h-4 animate-spin" />}
              Créer
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

export default function Apps() {
  const navigate = useNavigate();
  const [apps, setApps] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [showCreate, setShowCreate] = useState(false);
  const [busy, setBusy] = useState(null);

  const fetchApps = useCallback(async () => {
    try {
      const res = await listApps();
      const d = res.data?.data || res.data;
      const list = d?.apps || (Array.isArray(d) ? d : []);
      setApps(Array.isArray(list) ? list : []);
      setError(null);
    } catch (err) {
      setError(err.response?.data?.error || err.message || 'Erreur de chargement');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchApps();
  }, [fetchApps]);

  // Real-time state updates via WebSocket (no polling)
  useWebSocket({
    'app:state': (data) => {
      setApps(prev => prev.map(a =>
        a.slug === data.slug ? { ...a, state: data.state, port: data.port || a.port } : a
      ));
    },
  });

  async function handleControl(slug, action) {
    setBusy(`${slug}:${action}`);
    try {
      await controlApp(slug, action);
      await new Promise((r) => setTimeout(r, 800));
      await fetchApps();
    } catch (err) {
      setError(err.response?.data?.error || err.message || 'Action échouée');
    } finally {
      setBusy(null);
    }
  }

  async function handleDelete(slug) {
    if (!confirm(`Supprimer l'application "${slug}" ? Cette action est irréversible.`)) return;
    setBusy(`${slug}:delete`);
    try {
      await deleteApp(slug);
      await fetchApps();
    } catch (err) {
      setError(err.response?.data?.error || err.message || 'Suppression échouée');
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="flex flex-col h-full">
      <PageHeader title="Applications" icon={Boxes}>
        <button
          onClick={() => setShowCreate(true)}
          className="flex items-center gap-2 px-3 py-1.5 text-sm bg-blue-500 hover:bg-blue-600 text-white rounded-md"
        >
          <Plus className="w-4 h-4" />
          Nouvelle application
        </button>
      </PageHeader>

      <div className="flex-1 overflow-y-auto p-4 sm:p-6">
        {error && (
          <div className="mb-4 bg-red-500/10 border border-red-500/30 rounded-md px-4 py-3">
            <p className="text-sm text-red-400">{error}</p>
          </div>
        )}

        {loading ? (
          <div className="flex items-center justify-center py-16">
            <Loader2 className="w-6 h-6 animate-spin text-gray-500" />
          </div>
        ) : apps.length === 0 ? (
          <div className="bg-gray-800 border border-gray-700 rounded-lg p-12 text-center">
            <Boxes className="w-12 h-12 mx-auto text-gray-600 mb-3" />
            <p className="text-gray-400 mb-4">Aucune application déployée pour le moment.</p>
            <button
              onClick={() => setShowCreate(true)}
              className="inline-flex items-center gap-2 px-4 py-2 text-sm bg-blue-500 hover:bg-blue-600 text-white rounded-md"
            >
              <Plus className="w-4 h-4" />
              Créer la première application
            </button>
          </div>
        ) : (
          <div className="bg-gray-800 border border-gray-700 rounded-lg overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead className="bg-gray-900 text-gray-400 text-xs uppercase tracking-wider">
                  <tr>
                    <th className="px-4 py-3 text-left">Nom</th>
                    <th className="px-4 py-3 text-left">Slug</th>
                    <th className="px-4 py-3 text-left">Stack</th>
                    <th className="px-4 py-3 text-left">État</th>
                    <th className="px-4 py-3 text-left">Port</th>
                    <th className="px-4 py-3 text-left">Domaine</th>
                    <th className="px-4 py-3 text-left">Visibilité</th>
                    <th className="px-4 py-3 text-right">Actions</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-700">
                  {apps.map((app) => {
                    const slug = app.slug;
                    const state = (app.state || app.status || 'stopped').toLowerCase();
                    const isRunning = state === 'running';
                    const domain = app.domain || (slug ? `${slug}.mynetwk.biz` : '');
                    const url = domain ? `https://${domain}` : null;
                    return (
                      <tr key={slug} className="hover:bg-gray-700/30">
                        <td className="px-4 py-3">
                          <button
                            onClick={() => navigate(`/apps/${slug}`)}
                            className="text-white font-medium hover:text-blue-400 text-left"
                          >
                            {app.name || slug}
                          </button>
                        </td>
                        <td className="px-4 py-3 text-gray-400 font-mono text-xs">{slug}</td>
                        <td className="px-4 py-3 text-gray-300">{app.stack || '-'}</td>
                        <td className="px-4 py-3">{statusBadge(state)}</td>
                        <td className="px-4 py-3 text-gray-300 font-mono text-xs">{app.port || '-'}</td>
                        <td className="px-4 py-3">
                          {url ? (
                            <a
                              href={url}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="inline-flex items-center gap-1 text-blue-400 hover:text-blue-300 text-xs"
                            >
                              {domain}
                              <ExternalLink className="w-3 h-3" />
                            </a>
                          ) : (
                            <span className="text-gray-500">-</span>
                          )}
                        </td>
                        <td className="px-4 py-3">{visibilityBadge(app.visibility)}</td>
                        <td className="px-4 py-3">
                          <div className="flex items-center justify-end gap-1">
                            {!isRunning && (
                              <button
                                onClick={() => handleControl(slug, 'start')}
                                disabled={busy === `${slug}:start`}
                                title="Démarrer"
                                className="p-1.5 text-green-400 hover:bg-gray-700 rounded disabled:opacity-50"
                              >
                                {busy === `${slug}:start` ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
                              </button>
                            )}
                            {isRunning && (
                              <button
                                onClick={() => handleControl(slug, 'stop')}
                                disabled={busy === `${slug}:stop`}
                                title="Arrêter"
                                className="p-1.5 text-yellow-400 hover:bg-gray-700 rounded disabled:opacity-50"
                              >
                                {busy === `${slug}:stop` ? <Loader2 className="w-4 h-4 animate-spin" /> : <Square className="w-4 h-4" />}
                              </button>
                            )}
                            <button
                              onClick={() => handleControl(slug, 'restart')}
                              disabled={busy === `${slug}:restart`}
                              title="Redémarrer"
                              className="p-1.5 text-blue-400 hover:bg-gray-700 rounded disabled:opacity-50"
                            >
                              {busy === `${slug}:restart` ? <Loader2 className="w-4 h-4 animate-spin" /> : <RefreshCw className="w-4 h-4" />}
                            </button>
                            <button
                              onClick={() => navigate(`/apps/${slug}?tab=logs`)}
                              title="Logs"
                              className="p-1.5 text-gray-300 hover:bg-gray-700 rounded"
                            >
                              <ScrollText className="w-4 h-4" />
                            </button>
                            <button
                              onClick={() => handleDelete(slug)}
                              disabled={busy === `${slug}:delete`}
                              title="Supprimer"
                              className="p-1.5 text-red-400 hover:bg-gray-700 rounded disabled:opacity-50"
                            >
                              {busy === `${slug}:delete` ? <Loader2 className="w-4 h-4 animate-spin" /> : <Trash2 className="w-4 h-4" />}
                            </button>
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </div>

      {showCreate && (
        <CreateAppModal
          onClose={() => setShowCreate(false)}
          onCreated={() => {
            setShowCreate(false);
            fetchApps();
          }}
        />
      )}
    </div>
  );
}
