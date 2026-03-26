import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import {
  BookOpen, Search, Plus, Save, Check, ArrowLeft,
  FileText, Code, Cpu, StickyNote, Info,
} from 'lucide-react';
import PageHeader from '../components/PageHeader';
import Section from '../components/Section';
import Button from '../components/Button';
import { getDocsList, getDocsApp, createDocsApp, updateDocsSection, searchDocs } from '../api/client';

const SECTION_META = [
  { key: 'meta', label: 'Metadonnees', icon: Info },
  { key: 'structure', label: 'Structure', icon: FileText },
  { key: 'features', label: 'Features', icon: Code },
  { key: 'backend', label: 'Backend', icon: Cpu },
  { key: 'notes', label: 'Notes', icon: StickyNote },
];

function DocsList() {
  const [apps, setApps] = useState([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState('');
  const [searchResults, setSearchResults] = useState(null);
  const [newAppId, setNewAppId] = useState('');
  const [creating, setCreating] = useState(false);
  const navigate = useNavigate();

  const fetchDocs = useCallback(async () => {
    try {
      const res = await getDocsList();
      if (res.data.success) setApps(res.data.apps);
    } catch (e) {
      console.error('Failed to fetch docs:', e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchDocs(); }, [fetchDocs]);

  const handleSearch = useCallback(async () => {
    if (!search.trim()) { setSearchResults(null); return; }
    try {
      const res = await searchDocs(search);
      if (res.data.success) setSearchResults(res.data.results);
    } catch (e) {
      console.error('Search failed:', e);
    }
  }, [search]);

  const handleCreate = useCallback(async () => {
    if (!newAppId.trim()) return;
    setCreating(true);
    try {
      const res = await createDocsApp(newAppId.trim());
      if (res.data.success) {
        setNewAppId('');
        fetchDocs();
      }
    } catch (e) {
      console.error('Create failed:', e);
    } finally {
      setCreating(false);
    }
  }, [newAppId, fetchDocs]);

  if (loading) {
    return (
      <div>
        <PageHeader title="Documentation" icon={BookOpen} />
        <div className="p-8 text-center text-gray-400">Chargement...</div>
      </div>
    );
  }

  return (
    <div>
      <PageHeader title="Documentation" icon={BookOpen} />

      {/* Search + Create */}
      <Section>
        <div className="flex flex-col sm:flex-row gap-3">
          <div className="flex-1 flex gap-2">
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
              placeholder="Rechercher dans la doc..."
              className="flex-1 bg-gray-700 border border-gray-600 rounded px-3 py-2 text-sm text-white placeholder-gray-400 focus:outline-none focus:border-blue-500"
            />
            <Button onClick={handleSearch} variant="secondary">
              <Search className="w-4 h-4" />
            </Button>
          </div>
          <div className="flex gap-2">
            <input
              type="text"
              value={newAppId}
              onChange={(e) => setNewAppId(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
              placeholder="Nouvel app ID..."
              className="bg-gray-700 border border-gray-600 rounded px-3 py-2 text-sm text-white placeholder-gray-400 focus:outline-none focus:border-blue-500 w-40"
            />
            <Button onClick={handleCreate} loading={creating} variant="primary">
              <Plus className="w-4 h-4 mr-1" /> Creer
            </Button>
          </div>
        </div>
      </Section>

      {/* Search results */}
      {searchResults && (
        <Section title={`Resultats (${searchResults.length})`}>
          {searchResults.length === 0 ? (
            <p className="text-sm text-gray-400">Aucun resultat</p>
          ) : (
            <div className="space-y-1">
              {searchResults.map((r, i) => (
                <Link
                  key={i}
                  to={`/docs/${r.app_id}`}
                  className="block px-3 py-2 rounded hover:bg-gray-700 transition-colors"
                >
                  <span className="text-sm text-white">{r.app_id}</span>
                  <span className="text-xs text-gray-400 ml-2">{r.section}</span>
                </Link>
              ))}
            </div>
          )}
        </Section>
      )}

      {/* Apps list */}
      <Section title="Applications">
        {apps.length === 0 ? (
          <p className="text-sm text-gray-400">Aucune documentation. Creez-en une ci-dessus.</p>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3">
            {apps.map((app) => (
              <Link
                key={app.app_id}
                to={`/docs/${app.app_id}`}
                className="bg-gray-800 border border-gray-700 rounded-lg p-4 hover:border-gray-500 transition-colors group"
              >
                <div className="flex items-center justify-between mb-2">
                  <span className="font-medium text-sm truncate">{app.name}</span>
                  <span className={`text-xs px-2 py-0.5 rounded-full ${
                    app.filled === app.total
                      ? 'bg-green-900/30 text-green-400'
                      : app.filled > 0
                        ? 'bg-yellow-900/30 text-yellow-400'
                        : 'bg-gray-700 text-gray-400'
                  }`}>
                    {app.filled}/{app.total}
                  </span>
                </div>
                <div className="text-xs text-gray-500 font-mono">{app.app_id}</div>
                {/* Progress bar */}
                <div className="mt-2 w-full bg-gray-700 h-1 rounded-full overflow-hidden">
                  <div
                    className={`h-1 rounded-full transition-all ${
                      app.filled === app.total ? 'bg-green-400' : app.filled > 0 ? 'bg-yellow-400' : 'bg-gray-600'
                    }`}
                    style={{ width: `${(app.filled / app.total) * 100}%` }}
                  />
                </div>
              </Link>
            ))}
          </div>
        )}
      </Section>
    </div>
  );
}

function DocsDetail() {
  const { appId } = useParams();
  const navigate = useNavigate();
  const [data, setData] = useState(null);
  const [loading, setLoading] = useState(true);
  const [activeTab, setActiveTab] = useState('meta');
  const [editContent, setEditContent] = useState('');
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [dirty, setDirty] = useState(false);

  const fetchData = useCallback(async () => {
    try {
      const res = await getDocsApp(appId);
      if (res.data.success) {
        setData(res.data);
        // Set initial content for active tab
        if (activeTab === 'meta') {
          setEditContent(JSON.stringify(res.data.meta, null, 2));
        } else {
          setEditContent(res.data.sections[activeTab] || '');
        }
        setDirty(false);
      }
    } catch (e) {
      console.error('Failed to fetch docs:', e);
    } finally {
      setLoading(false);
    }
  }, [appId]);

  useEffect(() => { fetchData(); }, [fetchData]);

  // Update editContent when switching tabs
  useEffect(() => {
    if (!data) return;
    if (activeTab === 'meta') {
      setEditContent(JSON.stringify(data.meta, null, 2));
    } else {
      setEditContent(data.sections[activeTab] || '');
    }
    setDirty(false);
    setSaved(false);
  }, [activeTab, data]);

  const handleSave = useCallback(async () => {
    setSaving(true);
    try {
      await updateDocsSection(appId, activeTab, editContent);
      setSaved(true);
      setDirty(false);
      // Update local data
      setData(prev => {
        if (!prev) return prev;
        if (activeTab === 'meta') {
          try { return { ...prev, meta: JSON.parse(editContent) }; } catch { return prev; }
        }
        return { ...prev, sections: { ...prev.sections, [activeTab]: editContent } };
      });
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      console.error('Save failed:', e);
    } finally {
      setSaving(false);
    }
  }, [appId, activeTab, editContent]);

  // Ctrl+S shortcut
  useEffect(() => {
    const handler = (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        if (dirty) handleSave();
      }
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [dirty, handleSave]);

  if (loading) {
    return (
      <div>
        <PageHeader title="Documentation" icon={BookOpen} />
        <div className="p-8 text-center text-gray-400">Chargement...</div>
      </div>
    );
  }

  if (!data) {
    return (
      <div>
        <PageHeader title="Documentation" icon={BookOpen} />
        <div className="p-8 text-center text-gray-400">Documentation introuvable</div>
      </div>
    );
  }

  return (
    <div>
      <PageHeader title={data.meta?.name || appId} icon={BookOpen} />

      {/* Back + App ID */}
      <Section>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => navigate('/docs')} className="p-1 text-gray-400 hover:text-white transition-colors">
              <ArrowLeft className="w-5 h-5" />
            </button>
            <span className="text-sm text-gray-400 font-mono">{appId}</span>
          </div>
          <Button
            onClick={handleSave}
            loading={saving}
            disabled={!dirty}
            variant={saved ? 'success' : 'primary'}
          >
            {saved ? <><Check className="w-4 h-4 mr-1" /> Enregistre</> : <><Save className="w-4 h-4 mr-1" /> Enregistrer</>}
          </Button>
        </div>
      </Section>

      {/* Tabs */}
      <Section flush>
        <div className="flex border-b border-gray-700 overflow-x-auto">
          {SECTION_META.map(({ key, label, icon: Icon }) => (
            <button
              key={key}
              onClick={() => setActiveTab(key)}
              className={`flex items-center gap-1.5 px-4 py-2.5 text-sm whitespace-nowrap border-b-2 transition-colors ${
                activeTab === key
                  ? 'border-blue-400 text-blue-400'
                  : 'border-transparent text-gray-400 hover:text-gray-200'
              }`}
            >
              <Icon className="w-4 h-4" />
              {label}
            </button>
          ))}
        </div>

        {/* Editor */}
        <div className="p-4">
          <textarea
            value={editContent}
            onChange={(e) => { setEditContent(e.target.value); setDirty(true); setSaved(false); }}
            className="w-full h-[60vh] bg-gray-900 border border-gray-700 rounded-lg p-4 text-sm text-gray-200 font-mono resize-y focus:outline-none focus:border-blue-500"
            placeholder={activeTab === 'meta' ? '{"name": "", "stack": "", "description": "", "logo": ""}' : 'Contenu Markdown...'}
            spellCheck={false}
          />
        </div>
      </Section>
    </div>
  );
}

function Docs() {
  const { appId } = useParams();
  return appId ? <DocsDetail /> : <DocsList />;
}

export default Docs;
