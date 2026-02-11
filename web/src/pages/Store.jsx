import { useState, useEffect, useCallback } from 'react';
import {
  Store as StoreIcon, Package, ArrowLeft, Download,
  RefreshCw, Loader2, Tag, FileText, Smartphone
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

        <Section title={selectedApp.name}>
          <div className="flex items-center gap-6 text-sm">
            <div className="flex items-center gap-1.5">
              <Package className="w-3.5 h-3.5 text-gray-500" />
              <span className="text-white font-mono">{selectedApp.slug}</span>
            </div>
            <div className="flex items-center gap-1.5">
              <Tag className="w-3.5 h-3.5 text-gray-500" />
              <span className="text-gray-300">{selectedApp.category || 'other'}</span>
            </div>
            <span className="text-gray-400">{selectedApp.releases?.length || 0} release{(selectedApp.releases?.length || 0) !== 1 ? 's' : ''}</span>
            {selectedApp.description && (
              <span className="text-gray-500">{selectedApp.description}</span>
            )}
          </div>
        </Section>

        <Section title={`Releases (${releases.length})`}>
          {releases.length === 0 ? (
            <p className="text-sm text-gray-500">Aucune release.</p>
          ) : (
            <div className="-mx-6 -my-3 overflow-x-auto">
              <table className="w-full text-left">
                <thead className="bg-gray-800/60 border-b border-gray-700">
                  <tr>
                    <th className="px-3 py-2 text-xs font-medium text-gray-400 uppercase">Version</th>
                    <th className="px-3 py-2 text-xs font-medium text-gray-400 uppercase">Date</th>
                    <th className="px-3 py-2 text-xs font-medium text-gray-400 uppercase">Taille</th>
                    <th className="px-3 py-2 text-xs font-medium text-gray-400 uppercase">Changelog</th>
                    <th className="px-3 py-2 text-xs font-medium text-gray-400 uppercase">SHA-256</th>
                    <th className="px-3 py-2 text-xs font-medium text-gray-400 uppercase"></th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-700/50">
                  {releases.map((rel) => (
                    <tr key={rel.version} className="bg-gray-800 hover:bg-gray-700/50">
                      <td className="px-3 py-2 text-sm font-semibold text-blue-400">
                        <div className="flex items-center gap-1.5">
                          <Tag className="w-3.5 h-3.5" />
                          v{rel.version}
                        </div>
                      </td>
                      <td className="px-3 py-2 text-xs text-gray-400">
                        {new Date(rel.created_at).toLocaleDateString('fr-FR')}
                      </td>
                      <td className="px-3 py-2 text-sm text-gray-400">{formatSize(rel.size_bytes)}</td>
                      <td className="px-3 py-2 text-sm text-gray-400 max-w-xs truncate">
                        {rel.changelog || '--'}
                      </td>
                      <td className="px-3 py-2 text-xs text-gray-600 font-mono max-w-[120px] truncate">
                        {rel.sha256}
                      </td>
                      <td className="px-3 py-2">
                        <button
                          onClick={() => downloadStoreRelease(selectedApp.slug, rel.version)}
                          className="p-1.5 text-gray-400 hover:bg-gray-600/20 hover:text-white transition-colors"
                          title="Download"
                        >
                          <Download className="w-3.5 h-3.5" />
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
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
        <div className="flex items-center justify-between">
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
          <a
            href="/api/store/client/apk"
            download
            className="flex items-center gap-2 px-3 py-1.5 text-sm font-medium text-white bg-blue-600 hover:bg-blue-500 rounded transition-colors"
          >
            <Smartphone className="w-4 h-4" />
            Installer l'app Android
          </a>
        </div>
      </Section>

      <Section title={`Applications (${apps.length})`}>
        {apps.length === 0 ? (
          <div className="text-center py-4 text-gray-400 text-sm">
            Aucune application. Les publications sont gerees via MCP.
          </div>
        ) : (
          <div className="-mx-6 -my-3 overflow-x-auto">
            <table className="w-full text-left">
              <thead className="bg-gray-800/60 border-b border-gray-700">
                <tr>
                  <th className="px-3 py-2 text-xs font-medium text-gray-400 uppercase">Nom</th>
                  <th className="px-3 py-2 text-xs font-medium text-gray-400 uppercase">Categorie</th>
                  <th className="px-3 py-2 text-xs font-medium text-gray-400 uppercase">Version</th>
                  <th className="px-3 py-2 text-xs font-medium text-gray-400 uppercase">Taille</th>
                  <th className="px-3 py-2 text-xs font-medium text-gray-400 uppercase">Releases</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-700/50">
                {apps.map((app) => (
                  <tr
                    key={app.slug}
                    onClick={() => selectApp(app.slug)}
                    className="bg-gray-800 hover:bg-gray-700/50 cursor-pointer transition-colors"
                  >
                    <td className="px-3 py-2 text-sm font-medium text-white">
                      <div className="flex items-center gap-2">
                        <Package className="w-4 h-4 flex-shrink-0 text-blue-400" />
                        {app.name}
                      </div>
                    </td>
                    <td className="px-3 py-2 text-sm text-gray-300">{app.category || 'other'}</td>
                    <td className="px-3 py-2 text-sm font-mono text-gray-300">
                      {app.latest_version ? `v${app.latest_version}` : '--'}
                    </td>
                    <td className="px-3 py-2 text-sm text-gray-400">
                      {app.latest_size_bytes ? formatSize(app.latest_size_bytes) : '--'}
                    </td>
                    <td className="px-3 py-2 text-sm text-gray-400">
                      {app.release_count}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Section>

      {loadingDetail && (
        <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/40">
          <Loader2 className="w-8 h-8 text-blue-400 animate-spin" />
        </div>
      )}
    </div>
  );
}

export default Store;
