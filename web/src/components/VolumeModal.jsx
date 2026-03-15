import { useState, useEffect } from 'react';
import { X, HardDrive, Plus, Trash2, Loader2, Lock, Database } from 'lucide-react';
import Button from './Button';
import {
  getContainerVolumes,
  attachContainerVolume,
  detachContainerVolume,
} from '../api/client';

function VolumeModal({ container, onClose }) {
  const [volumes, setVolumes] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [saving, setSaving] = useState(false);
  const [showForm, setShowForm] = useState(false);
  const [confirmDetach, setConfirmDetach] = useState(null);
  const [form, setForm] = useState({
    name: '',
    source_path: '',
    mount_point: '',
    read_only: false,
    zfs_dataset: '',
    zfs_quota: '',
  });

  async function fetchVolumes() {
    try {
      setLoading(true);
      setError(null);
      const res = await getContainerVolumes(container.id);
      setVolumes(res.data.volumes || res.data || []);
    } catch (err) {
      setError(err.response?.data?.error || 'Erreur de chargement');
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    fetchVolumes();
  }, [container.id]);

  async function handleAttach() {
    if (!form.name || !form.source_path || !form.mount_point) return;
    setSaving(true);
    try {
      const payload = {
        name: form.name,
        source_path: form.source_path,
        mount_point: form.mount_point,
        read_only: form.read_only,
      };
      if (form.zfs_dataset) payload.zfs_dataset = form.zfs_dataset;
      if (form.zfs_quota) payload.zfs_quota = form.zfs_quota;

      const res = await attachContainerVolume(container.id, payload);
      if (res.data.success !== false) {
        setShowForm(false);
        setForm({ name: '', source_path: '', mount_point: '', read_only: false, zfs_dataset: '', zfs_quota: '' });
        fetchVolumes();
      } else {
        setError(res.data.error || 'Erreur');
      }
    } catch (err) {
      setError(err.response?.data?.error || 'Erreur');
    } finally {
      setSaving(false);
    }
  }

  async function handleDetach(volId) {
    setSaving(true);
    try {
      const res = await detachContainerVolume(container.id, volId);
      if (res.data.success !== false) {
        setConfirmDetach(null);
        fetchVolumes();
      } else {
        setError(res.data.error || 'Erreur');
      }
    } catch (err) {
      setError(err.response?.data?.error || 'Erreur');
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
      <div className="bg-gray-800 p-6 w-full max-w-lg border border-gray-700 rounded-lg max-h-[90vh] overflow-y-auto">
        {/* Header */}
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-2">
            <HardDrive className="w-5 h-5 text-blue-400" />
            <h2 className="text-xl font-bold">Volumes</h2>
            <span className="text-sm text-gray-500">{container.name || container.slug}</span>
          </div>
          <button onClick={onClose} className="p-1 text-gray-400 hover:text-white">
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Error */}
        {error && (
          <div className="mb-4 p-3 bg-red-900/30 border border-red-700/50 rounded text-red-400 text-sm">
            {error}
          </div>
        )}

        {/* Volume List */}
        {loading ? (
          <div className="flex items-center justify-center py-8">
            <Loader2 className="w-6 h-6 animate-spin text-gray-400" />
          </div>
        ) : volumes.length === 0 ? (
          <div className="text-center py-8 text-gray-500">
            <HardDrive className="w-10 h-10 mx-auto mb-2 opacity-40" />
            <p className="text-sm">Aucun volume attache</p>
          </div>
        ) : (
          <div className="space-y-2 mb-4">
            {volumes.map(vol => (
              <div
                key={vol.id}
                className="p-3 bg-gray-900 border border-gray-700 rounded flex items-start justify-between gap-2"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 mb-1">
                    <span className="font-medium text-sm truncate">{vol.name}</span>
                    {vol.read_only && (
                      <span className="flex items-center gap-0.5 text-[10px] px-1.5 py-0.5 bg-yellow-900/40 text-yellow-400 rounded">
                        <Lock className="w-2.5 h-2.5" />
                        RO
                      </span>
                    )}
                    {vol.zfs_dataset && (
                      <span className="flex items-center gap-0.5 text-[10px] px-1.5 py-0.5 bg-blue-900/40 text-blue-400 rounded">
                        <Database className="w-2.5 h-2.5" />
                        ZFS
                      </span>
                    )}
                  </div>
                  <div className="text-xs text-gray-500 font-mono truncate">
                    {vol.source_path} <span className="text-gray-600 mx-1">&rarr;</span> {vol.mount_point}
                  </div>
                  {vol.zfs_dataset && (
                    <div className="text-xs text-gray-600 font-mono mt-0.5">
                      {vol.zfs_dataset}{vol.zfs_quota ? ` (${vol.zfs_quota})` : ''}
                    </div>
                  )}
                </div>
                <div className="shrink-0">
                  {confirmDetach === vol.id ? (
                    <div className="flex items-center gap-1">
                      <button
                        onClick={() => handleDetach(vol.id)}
                        disabled={saving}
                        className="px-2 py-1 text-xs bg-red-600 hover:bg-red-700 text-white rounded transition-colors disabled:opacity-50"
                      >
                        Confirmer
                      </button>
                      <button
                        onClick={() => setConfirmDetach(null)}
                        className="px-2 py-1 text-xs text-gray-400 hover:text-white transition-colors"
                      >
                        Non
                      </button>
                    </div>
                  ) : (
                    <button
                      onClick={() => setConfirmDetach(vol.id)}
                      className="p-1 text-gray-500 hover:text-red-400 transition-colors"
                      title="Detacher"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Attach Form */}
        {showForm ? (
          <div className="border border-gray-700 rounded p-4 space-y-3 mt-4">
            <h3 className="text-sm font-semibold text-gray-300 mb-2">Attacher un volume</h3>
            <div>
              <label className="block text-xs text-gray-400 mb-1">Nom</label>
              <input
                type="text"
                placeholder="data"
                value={form.name}
                onChange={e => setForm({ ...form, name: e.target.value })}
                className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm"
              />
            </div>
            <div>
              <label className="block text-xs text-gray-400 mb-1">Chemin source (hote)</label>
              <input
                type="text"
                placeholder="/data/volumes/my-app"
                value={form.source_path}
                onChange={e => setForm({ ...form, source_path: e.target.value })}
                className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm font-mono"
              />
            </div>
            <div>
              <label className="block text-xs text-gray-400 mb-1">Point de montage (conteneur)</label>
              <input
                type="text"
                placeholder="/data"
                value={form.mount_point}
                onChange={e => setForm({ ...form, mount_point: e.target.value })}
                className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm font-mono"
              />
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="block text-xs text-gray-400 mb-1">ZFS dataset (optionnel)</label>
                <input
                  type="text"
                  placeholder="pool/volumes/app"
                  value={form.zfs_dataset}
                  onChange={e => setForm({ ...form, zfs_dataset: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm font-mono"
                />
              </div>
              <div>
                <label className="block text-xs text-gray-400 mb-1">ZFS quota (optionnel)</label>
                <input
                  type="text"
                  placeholder="10G"
                  value={form.zfs_quota}
                  onChange={e => setForm({ ...form, zfs_quota: e.target.value })}
                  className="w-full px-3 py-2 bg-gray-900 border border-gray-600 rounded text-sm font-mono"
                />
              </div>
            </div>
            <label className="flex items-center gap-2 text-xs cursor-pointer">
              <input
                type="checkbox"
                checked={form.read_only}
                onChange={e => setForm({ ...form, read_only: e.target.checked })}
                className="rounded"
              />
              Lecture seule
            </label>
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="secondary" onClick={() => setShowForm(false)} disabled={saving}>
                Annuler
              </Button>
              <Button
                onClick={handleAttach}
                loading={saving}
                disabled={saving || !form.name || !form.source_path || !form.mount_point}
              >
                Attacher
              </Button>
            </div>
          </div>
        ) : (
          <button
            onClick={() => setShowForm(true)}
            className="w-full mt-2 py-2 border border-dashed border-gray-600 rounded text-sm text-gray-400 hover:text-white hover:border-gray-500 transition-colors flex items-center justify-center gap-1.5"
          >
            <Plus className="w-4 h-4" />
            Attacher un volume
          </button>
        )}

        {/* Footer */}
        <div className="flex justify-end mt-6">
          <Button variant="secondary" onClick={onClose}>Fermer</Button>
        </div>
      </div>
    </div>
  );
}

export default VolumeModal;
