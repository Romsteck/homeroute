import { useState, useEffect, useCallback } from 'react';
import {
  Store as StoreIcon, Package, ArrowLeft, Download,
  RefreshCw, Loader2, Tag, FileText
} from 'lucide-react';
import PageHeader from '../components/PageHeader';
import Section from '../components/Section';
import Button from '../components/Button';
import {
  getStoreApps, getStoreApp, downloadStoreRelease
} from '../api/client';

const formatSize = (bytes) =>
  bytes >= 1e6 ? (bytes / 1e6).toFixed(1) + ' MB' : (bytes / 1e3).toFixed(0) + ' KB';

function Store() {
  const [apps, setApps] = useState([]);
  const [loading, setLoading] = useState(true);
  const [message, setMessage] = useState(null);
  const [selectedApp, setSelectedApp] = useState(null);
  const [loadingDetail, setLoadingDetail] = useState(false);

  const fetchApps = useCallback(async () => {
    try {
      const res = await getStoreApps();
      setApps(res.data?.apps || []);
    } catch (err) {
      setMessage({ type: 'error', text: 'Erreur lors du chargement du store' });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchApps(); }, [fetchApps]);

  const selectApp = async (slug) => {
    setLoadingDetail(true);
    try {
      const res = await getStoreApp(slug);
      setSelectedApp(res.data?.app || null);
    } catch {
      setMessage({ type: 'error', text: 'Erreur lors du chargement des details' });
    } finally {
      setLoadingDetail(false);
    }
  };

  const goBack = () => setSelectedApp(null);

  const handleRefresh = () => {
    setLoading(true);
    fetchApps();
  };

  const totalReleases = apps.reduce((sum, a) => sum + (a.release_count || 0), 0);

  if (loading) {
    return (
      <div className="h-full flex flex-col">
        <PageHeader icon={StoreIcon} title="Store" />
        <div className="flex-1 flex items-center justify-center">
          <Loader2 className="w-8 h-8 text-blue-400 animate-spin" />
        </div>
      </div>
    );
  }

  // Detail view
  if (selectedApp) {
    const releases = [...(selectedApp.releases || [])].reverse();
    return (
      <div className="h-full flex flex-col overflow-y-auto">
        <PageHeader icon={StoreIcon} title="Store">
          <Button variant="secondary" onClick={goBack}>
            <ArrowLeft className="w-4 h-4" /> Retour
          </Button>
          {releases.length > 0 && (
            <Button onClick={() => downloadStoreRelease(selectedApp.slug, releases[0].version)}>
              <Download className="w-4 h-4" /> Derniere version
            </Button>
          )}
        </PageHeader>

        <Section title="Informations">
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
            <div>
              <span className="text-gray-500">Nom</span>
              <p className="text-white font-medium">{selectedApp.name}</p>
            </div>
            <div>
              <span className="text-gray-500">Slug</span>
              <p className="text-white font-mono">{selectedApp.slug}</p>
            </div>
            <div>
              <span className="text-gray-500">Categorie</span>
              <p className="text-white">{selectedApp.category || 'other'}</p>
            </div>
            <div>
              <span className="text-gray-500">Releases</span>
              <p className="text-white">{selectedApp.releases?.length || 0}</p>
            </div>
          </div>
          {selectedApp.description && (
            <p className="text-sm text-gray-400 mt-3">{selectedApp.description}</p>
          )}
        </Section>

        <Section title="Releases">
          {releases.length === 0 ? (
            <p className="text-sm text-gray-500">Aucune release.</p>
          ) : (
            <div className="space-y-2">
              {releases.map((rel) => (
                <div key={rel.version} className="bg-gray-800 border border-gray-700 rounded-lg p-4">
                  <div className="flex items-center justify-between mb-2">
                    <div className="flex items-center gap-3">
                      <span className="flex items-center gap-1.5 text-sm font-semibold text-blue-400">
                        <Tag className="w-3.5 h-3.5" />
                        v{rel.version}
                      </span>
                      <span className="text-xs text-gray-500">
                        {new Date(rel.created_at).toLocaleDateString('fr-FR')}
                      </span>
                      <span className="text-xs text-gray-500">{formatSize(rel.size_bytes)}</span>
                    </div>
                    <button
                      onClick={() => downloadStoreRelease(selectedApp.slug, rel.version)}
                      className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs text-gray-300 hover:text-white hover:bg-gray-700 transition-colors"
                    >
                      <Download className="w-3.5 h-3.5" /> Download
                    </button>
                  </div>
                  {rel.changelog && (
                    <p className="text-sm text-gray-400 flex items-start gap-2">
                      <FileText className="w-3.5 h-3.5 mt-0.5 flex-shrink-0 text-gray-500" />
                      {rel.changelog}
                    </p>
                  )}
                  <p className="text-xs text-gray-600 mt-2 font-mono truncate">SHA-256: {rel.sha256}</p>
                </div>
              ))}
            </div>
          )}
        </Section>
      </div>
    );
  }

  // Catalog view
  return (
    <div className="h-full flex flex-col overflow-y-auto">
      <PageHeader icon={StoreIcon} title="Store">
        <Button variant="secondary" onClick={handleRefresh}>
          <RefreshCw className="w-4 h-4" />
        </Button>
      </PageHeader>

      {message && (
        <div className={`mx-6 mt-3 text-sm rounded px-3 py-2 ${
          message.type === 'error'
            ? 'text-red-400 bg-red-900/20 border border-red-800'
            : 'text-green-400 bg-green-900/20 border border-green-800'
        }`}>
          {message.text}
        </div>
      )}

      <Section>
        <div className="flex items-center gap-6 text-sm">
          <div className="flex items-center gap-2">
            <Package className="w-4 h-4 text-blue-400" />
            <span className="text-gray-400">{apps.length} application{apps.length !== 1 ? 's' : ''}</span>
          </div>
          <div className="flex items-center gap-2">
            <Tag className="w-4 h-4 text-green-400" />
            <span className="text-gray-400">{totalReleases} release{totalReleases !== 1 ? 's' : ''}</span>
          </div>
        </div>
      </Section>

      <div className="flex-1 px-6 py-4">
        {apps.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <div className="text-center">
              <StoreIcon className="w-12 h-12 text-gray-600 mx-auto mb-3" />
              <p className="text-gray-400">Aucune application.</p>
              <p className="text-sm text-gray-500 mt-1">Les publications sont gerees via MCP.</p>
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {apps.map((app) => (
              <div
                key={app.slug}
                onClick={() => selectApp(app.slug)}
                className="bg-gray-800 border border-gray-700 rounded-lg p-4 hover:border-blue-500/50 hover:bg-gray-800/80 transition-colors cursor-pointer"
              >
                <div className="flex items-start gap-3">
                  <div className="w-10 h-10 rounded-lg bg-blue-600/20 flex items-center justify-center flex-shrink-0">
                    <Package className="w-5 h-5 text-blue-400" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <h3 className="text-sm font-semibold text-white truncate">{app.name}</h3>
                    <p className="text-xs text-gray-500">{app.category || 'other'}</p>
                  </div>
                </div>
                <div className="mt-3 flex items-center justify-between text-xs text-gray-400">
                  <span>
                    {app.latest_version ? `v${app.latest_version}` : 'N/A'}
                    {app.latest_size_bytes ? ` · ${formatSize(app.latest_size_bytes)}` : ''}
                  </span>
                  <span>{app.release_count} release{app.release_count !== 1 ? 's' : ''}</span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {loadingDetail && (
        <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/40">
          <Loader2 className="w-8 h-8 text-blue-400 animate-spin" />
        </div>
      )}
    </div>
  );
}

export default Store;
