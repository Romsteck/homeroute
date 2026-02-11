import { useState, useEffect, useCallback } from 'react';
import { Cloud, Power, PowerOff, RefreshCw, Server, Activity, Wifi, Upload, ArrowUpCircle } from 'lucide-react';
import Button from '../components/Button';
import StatusBadge from '../components/StatusBadge';
import PageHeader from '../components/PageHeader';
import Section from '../components/Section';
import ConfirmModal from '../components/ConfirmModal';
import useWebSocket from '../hooks/useWebSocket';
import {
  getCloudRelayStatus, enableCloudRelay, disableCloudRelay,
  bootstrapCloudRelay, pushCloudRelayUpdate,
} from '../api/client';

function CloudRelay() {
  const [status, setStatus] = useState(null);
  const [loading, setLoading] = useState(true);
  const [enabling, setEnabling] = useState(false);
  const [disabling, setDisabling] = useState(false);
  const [bootstrapping, setBootstrapping] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [updateLog, setUpdateLog] = useState(null);
  const [bootstrapLog, setBootstrapLog] = useState(null);
  const [showDisableConfirm, setShowDisableConfirm] = useState(false);
  const [showBootstrapForm, setShowBootstrapForm] = useState(false);
  const [bootstrapForm, setBootstrapForm] = useState({ host: '', ssh_user: 'root', ssh_port: '22', ssh_password: '' });

  const fetchStatus = useCallback(async () => {
    try {
      const res = await getCloudRelayStatus();
      setStatus(res.data);
    } catch (error) {
      console.error('Error fetching relay status:', error);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchStatus();
    const interval = setInterval(fetchStatus, 10000);
    return () => clearInterval(interval);
  }, [fetchStatus]);

  // Real-time WebSocket updates
  useWebSocket({
    'cloud_relay:status': (data) => {
      setStatus(prev => ({
        ...prev,
        status: data.status?.toLowerCase() || prev?.status,
        latency_ms: data.latency_ms ?? prev?.latency_ms,
        active_streams: data.active_streams ?? prev?.active_streams,
      }));
    },
  });

  async function handleEnable() {
    setEnabling(true);
    try {
      const res = await enableCloudRelay();
      if (res.data.success) {
        await fetchStatus();
      }
    } catch (error) {
      console.error('Enable error:', error);
    } finally {
      setEnabling(false);
    }
  }

  async function handleDisable() {
    setDisabling(true);
    try {
      const res = await disableCloudRelay();
      if (res.data.success) {
        await fetchStatus();
      }
    } catch (error) {
      console.error('Disable error:', error);
    } finally {
      setDisabling(false);
      setShowDisableConfirm(false);
    }
  }

  async function handleBootstrap() {
    if (!bootstrapForm.host.trim() || !bootstrapForm.ssh_user.trim()) return;
    setBootstrapping(true);
    setBootstrapLog(null);
    try {
      const payload = {
        host: bootstrapForm.host.trim(),
        ssh_user: bootstrapForm.ssh_user.trim(),
        ssh_port: parseInt(bootstrapForm.ssh_port) || 22,
      };
      if (bootstrapForm.ssh_password.trim()) {
        payload.ssh_password = bootstrapForm.ssh_password.trim();
      }
      const res = await bootstrapCloudRelay(payload);
      if (res.data.success) {
        setBootstrapLog({ success: true, message: res.data.message, vps_ipv4: res.data.vps_ipv4 });
        setShowBootstrapForm(false);
        await fetchStatus();
      } else {
        setBootstrapLog({ success: false, message: res.data.error || 'Erreur inconnue' });
      }
    } catch (error) {
      const msg = error.response?.data || error.message;
      setBootstrapLog({ success: false, message: typeof msg === 'string' ? msg : JSON.stringify(msg) });
    } finally {
      setBootstrapping(false);
    }
  }

  async function handleUpdate() {
    setUpdating(true);
    setUpdateLog(null);
    try {
      const res = await pushCloudRelayUpdate();
      if (res.data.success) {
        setUpdateLog({ success: true, message: res.data.message });
      } else {
        setUpdateLog({ success: false, message: res.data.error || 'Erreur inconnue' });
      }
    } catch (error) {
      const msg = error.response?.data || error.message;
      setUpdateLog({ success: false, message: typeof msg === 'string' ? msg : JSON.stringify(msg) });
    } finally {
      setUpdating(false);
    }
  }

  const isConnected = status?.status === 'connected';
  const isEnabled = status?.enabled;
  const isBootstrapped = !!status?.vps_host;

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-400"></div>
      </div>
    );
  }

  return (
    <div>
      <PageHeader title="Cloud Relay" icon={Cloud}>
        <div className="flex items-center gap-2">
          {isEnabled ? (
            <StatusBadge status={isConnected ? 'up' : 'unknown'}>
              {status?.status || 'disconnected'}
            </StatusBadge>
          ) : (
            <StatusBadge status="down">Desactive</StatusBadge>
          )}
          <Button onClick={fetchStatus} variant="secondary">
            <RefreshCw className="w-4 h-4" />
          </Button>
        </div>
      </PageHeader>

      {/* Status Overview */}
      <Section title="Tunnel QUIC">
        <div className="flex items-center gap-6 text-sm">
          <div className="flex items-center gap-2">
            <div className={`w-2.5 h-2.5 rounded-full ${isConnected ? 'bg-green-500 animate-pulse' : isEnabled ? 'bg-yellow-500' : 'bg-gray-600'}`} />
            <span className={`font-semibold ${isConnected ? 'text-green-400' : isEnabled ? 'text-yellow-400' : 'text-gray-500'}`}>
              {isConnected ? 'Connecte' : isEnabled ? (status?.status || 'Deconnecte') : 'Desactive'}
            </span>
            <span className="text-xs text-gray-500">
              {isEnabled ? 'relay VPS' : 'direct IPv6'}
            </span>
          </div>
          <div className="flex items-center gap-1.5 text-gray-400">
            <Server className="w-3.5 h-3.5 text-blue-400" />
            <span className="font-mono text-blue-400">{status?.vps_host || '-'}</span>
            {status?.vps_ipv4 && <span className="font-mono text-xs">({status.vps_ipv4})</span>}
          </div>
          <div className="flex items-center gap-1.5">
            <Wifi className="w-3.5 h-3.5 text-gray-500" />
            <span className={`font-semibold ${status?.latency_ms != null ? (status.latency_ms < 50 ? 'text-green-400' : status.latency_ms < 100 ? 'text-yellow-400' : 'text-red-400') : 'text-gray-600'}`}>
              {status?.latency_ms != null ? `${status.latency_ms}ms` : '-'}
            </span>
          </div>
          <div className="flex items-center gap-1.5">
            <Activity className="w-3.5 h-3.5 text-gray-500" />
            <span className="font-semibold text-blue-400">{status?.active_streams ?? '-'}</span>
            <span className="text-xs text-gray-500">streams</span>
          </div>
        </div>
      </Section>

      {/* Actions */}
      <Section title="Actions" contrast>
        <div className="flex flex-wrap items-center gap-2">
          {!isEnabled ? (
            <Button onClick={handleEnable} loading={enabling} variant="success" size="sm" disabled={!isBootstrapped}>
              <Power className="w-3.5 h-3.5" />
              Activer le relay
            </Button>
          ) : (
            <Button onClick={() => setShowDisableConfirm(true)} variant="danger" size="sm">
              <PowerOff className="w-3.5 h-3.5" />
              Desactiver le relay
            </Button>
          )}
          <Button onClick={() => {
            if (!showBootstrapForm && status) {
              setBootstrapForm(f => ({
                ...f,
                host: status.vps_host || f.host,
                ssh_user: status.ssh_user || f.ssh_user,
                ssh_port: status.ssh_port ? String(status.ssh_port) : f.ssh_port,
              }));
            }
            setShowBootstrapForm(!showBootstrapForm);
          }} variant="primary" size="sm">
            <Upload className="w-3.5 h-3.5" />
            {isBootstrapped ? 'Re-bootstrap VPS' : 'Bootstrap VPS'}
          </Button>
          {isBootstrapped && isConnected && (
            <Button onClick={handleUpdate} loading={updating} variant="secondary" size="sm">
              <ArrowUpCircle className="w-3.5 h-3.5" />
              Mettre a jour l&apos;agent VPS
            </Button>
          )}
          {!isBootstrapped && !isEnabled && (
            <span className="text-xs text-gray-500">Commencez par bootstrapper une VPS pour activer le relay.</span>
          )}
        </div>

        {/* Bootstrap Form */}
        {showBootstrapForm && (
          <div className="mt-3 bg-gray-800 border border-gray-700 p-3 max-w-lg">
            <h3 className="text-sm font-semibold mb-2">Deployer hr-cloud-relay sur la VPS</h3>
            <p className="text-xs text-gray-400 mb-3">
              Le binaire sera compile, copie via SCP, et installe comme service systemd sur la VPS.
            </p>
            <div className="space-y-2">
              <div>
                <label className="block text-xs text-gray-400 mb-0.5">Hote VPS (IP ou hostname)</label>
                <input
                  type="text"
                  value={bootstrapForm.host}
                  onChange={(e) => setBootstrapForm(f => ({ ...f, host: e.target.value }))}
                  placeholder="vps.example.com"
                  className="w-full bg-gray-900 border border-gray-600 px-2 py-1.5 text-sm font-mono text-white focus:outline-none focus:border-blue-500"
                />
              </div>
              <div className="grid grid-cols-2 gap-2">
                <div>
                  <label className="block text-xs text-gray-400 mb-0.5">Utilisateur SSH</label>
                  <input
                    type="text"
                    value={bootstrapForm.ssh_user}
                    onChange={(e) => setBootstrapForm(f => ({ ...f, ssh_user: e.target.value }))}
                    className="w-full bg-gray-900 border border-gray-600 px-2 py-1.5 text-sm font-mono text-white focus:outline-none focus:border-blue-500"
                  />
                </div>
                <div>
                  <label className="block text-xs text-gray-400 mb-0.5">Port SSH</label>
                  <input
                    type="number"
                    value={bootstrapForm.ssh_port}
                    onChange={(e) => setBootstrapForm(f => ({ ...f, ssh_port: e.target.value }))}
                    className="w-full bg-gray-900 border border-gray-600 px-2 py-1.5 text-sm font-mono text-white focus:outline-none focus:border-blue-500"
                  />
                </div>
              </div>
              <div>
                <label className="block text-xs text-gray-400 mb-0.5">Mot de passe SSH</label>
                <input
                  type="password"
                  value={bootstrapForm.ssh_password}
                  onChange={(e) => setBootstrapForm(f => ({ ...f, ssh_password: e.target.value }))}
                  placeholder="Laisser vide si cle SSH"
                  className="w-full bg-gray-900 border border-gray-600 px-2 py-1.5 text-sm font-mono text-white focus:outline-none focus:border-blue-500"
                />
              </div>
              <div className="flex gap-2 pt-1">
                <Button onClick={handleBootstrap} loading={bootstrapping} variant="success" size="sm" disabled={!bootstrapForm.host.trim()}>
                  <Upload className="w-3.5 h-3.5" />
                  Deployer
                </Button>
                <Button onClick={() => setShowBootstrapForm(false)} variant="secondary" size="sm">
                  Annuler
                </Button>
              </div>
            </div>
          </div>
        )}

        {/* Bootstrap Result */}
        {bootstrapLog && (
          <div className={`mt-2 px-3 py-2 border ${bootstrapLog.success ? 'border-green-700 bg-green-900/20' : 'border-red-700 bg-red-900/20'}`}>
            <p className={`text-sm ${bootstrapLog.success ? 'text-green-400' : 'text-red-400'}`}>
              {bootstrapLog.message}
            </p>
            {bootstrapLog.vps_ipv4 && (
              <p className="text-xs text-gray-400 mt-0.5">IPv4 VPS: <span className="font-mono text-blue-400">{bootstrapLog.vps_ipv4}</span></p>
            )}
          </div>
        )}

        {/* Update Result */}
        {updateLog && (
          <div className={`mt-2 px-3 py-2 border ${updateLog.success ? 'border-green-700 bg-green-900/20' : 'border-red-700 bg-red-900/20'}`}>
            <p className={`text-sm ${updateLog.success ? 'text-green-400' : 'text-red-400'}`}>
              {updateLog.message}
            </p>
          </div>
        )}
      </Section>

      {/* Architecture Info */}
      <Section title="Architecture">
        <div className="font-mono text-xs text-gray-400">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="text-blue-400">Client</span>
            <span className="text-gray-600">&rarr;</span>
            <span className="text-orange-400">Cloudflare</span>
            <span className="text-gray-600">&rarr;</span>
            <span className="text-purple-400">VPS :443</span>
            <span className="text-green-400">=== QUIC ===&gt;</span>
            <span className="text-cyan-400">On-Prem</span>
            <span className="text-gray-600">&rarr;</span>
            <span className="text-green-400">Proxy</span>
          </div>
          <p className="text-gray-500 mt-1.5">TCP brut via tunnel QUIC multiplex. TLS termine on-prem. mTLS VPS/on-prem.</p>
        </div>
      </Section>

      {/* Disable Confirmation Modal */}
      <ConfirmModal
        isOpen={showDisableConfirm}
        onClose={() => setShowDisableConfirm(false)}
        onConfirm={handleDisable}
        title="Desactiver le Cloud Relay"
        message="Le DNS Cloudflare sera bascule vers le mode direct (AAAA IPv6). Le trafic externe ne passera plus par la VPS. Confirmer ?"
        confirmText="Desactiver"
        variant="danger"
        loading={disabling}
      />
    </div>
  );
}

export default CloudRelay;
