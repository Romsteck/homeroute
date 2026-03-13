import {
  Wifi,
  WifiOff,
  Clock,
  Loader2,
  Play,
  Square,
  Terminal,
  ArrowRightLeft,
  Key,
  Shield,
  AlertTriangle,
  HardDrive,
  ExternalLink,
} from 'lucide-react';

// Shared grid template — used by ContainerCard rows and column header in Containers.jsx
// 9 columns: Env | Status | Auth | Local | Acces | CPU | RAM | Host | Actions
export const CONTAINER_GRID = '50px 1.2fr 26px 26px 230px 55px 65px 1fr 140px';

const STATUS_BADGES = {
  connected: { color: 'text-green-400 bg-green-900/30', icon: Wifi, label: 'Connecte' },
  deploying: { color: 'text-blue-400 bg-blue-900/30', icon: Loader2, label: 'Deploiement', spin: true },
  pending: { color: 'text-yellow-400 bg-yellow-900/30', icon: Clock, label: 'En attente' },
  running: { color: 'text-yellow-400 bg-yellow-900/30', icon: Clock, label: 'En attente' },
  stopped: { color: 'text-gray-400 bg-gray-900/30', icon: Square, label: 'Arrete' },
  disconnected: { color: 'text-red-400 bg-red-900/30', icon: WifiOff, label: 'Deconnecte' },
  error: { color: 'text-red-400 bg-red-900/30', icon: AlertTriangle, label: 'Erreur' },
};

function StatusBadge({ status }) {
  const badge = STATUS_BADGES[status] || STATUS_BADGES.disconnected;
  const Icon = badge.icon;
  return (
    <span className={`flex items-center gap-1 text-xs px-2 py-0.5 ${badge.color}`}>
      <Icon className={`w-3 h-3 ${badge.spin ? 'animate-spin' : ''}`} />
      {badge.label}
    </span>
  );
}

function formatBytes(bytes) {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(0)) + ' ' + sizes[i];
}

function ContainerCard({
  container,
  baseDomain,
  metrics,
  migration,
  hosts,
  isHostOffline,
  onStart,
  onStop,
  onTerminal,
  onToggleSecurity,
  onMigrate,
  onMigrationDismiss,
  MigrationProgress,
}) {
  const displayStatus = container.agent_status || container.status;
  const isMigrating = !!migration;
  const isDev = container.environment !== 'production';
  const host = container.host_id && container.host_id !== 'local'
    ? hosts?.find(h => h.id === container.host_id)
    : null;

  const prodAppUrl = baseDomain && !isDev ? `${container.slug}.${baseDomain}` : null;

  const isConnected = displayStatus === 'connected';

  return (
    <div className={isHostOffline ? 'opacity-60' : ''}>
      {/* Desktop: grid row */}
      <div
        className="hidden lg:grid items-center gap-x-3 px-4 py-1.5 border-b border-gray-700/30 transition-[background-color] duration-500 ease-out hover:bg-gray-600/30 hover:duration-0"
        style={{ gridTemplateColumns: CONTAINER_GRID }}
      >
        {/* Env */}
        <span className={`text-xs px-1.5 py-0.5 font-medium text-center ${
          isDev ? 'bg-blue-100 text-blue-800' : 'bg-purple-100 text-purple-800'
        }`}>
          {isDev ? 'DEV' : 'PROD'}
        </span>

        {/* Status */}
        <StatusBadge status={displayStatus} />

        {/* Auth toggle */}
        <button
          onClick={() => onToggleSecurity(container.id, 'auth_required', !container.frontend?.auth_required)}
          className={`p-0.5 justify-self-center transition-colors ${
            container.frontend?.auth_required
              ? 'text-purple-400 hover:text-purple-300'
              : 'text-purple-400 opacity-30 hover:opacity-60'
          }`}
          title={container.frontend?.auth_required ? 'Auth requis (cliquer pour desactiver)' : 'Auth non requis (cliquer pour activer)'}
        >
          <Key className="w-3 h-3" />
        </button>

        {/* Local-only toggle */}
        <button
          onClick={() => onToggleSecurity(container.id, 'local_only', !container.frontend?.local_only)}
          className={`p-0.5 justify-self-center transition-colors ${container.frontend?.local_only ? 'text-yellow-400 hover:text-yellow-300' : 'text-yellow-400 opacity-30 hover:opacity-60'}`}
          title={container.frontend?.local_only ? 'Local uniquement (cliquer pour desactiver)' : 'Acces externe (cliquer pour restreindre au local)'}
        >
          <Shield className="w-3 h-3" />
        </button>

        {/* Access buttons */}
        <div className="flex items-center gap-1 justify-self-start">
          {prodAppUrl && (
            <a href={`https://${prodAppUrl}`} target="_blank" rel="noopener noreferrer"
              className="inline-flex items-center gap-0.5 px-1.5 py-0.5 text-xs text-green-400 hover:text-green-300 bg-green-900/20 rounded">
              <ExternalLink className="w-3 h-3" /> APP
            </a>
          )}
        </div>

        {/* CPU */}
        <span className={`font-mono text-xs ${
          isConnected && metrics?.cpuPercent > 80 ? 'text-red-400' :
          isConnected && metrics?.cpuPercent > 50 ? 'text-yellow-400' :
          isConnected && metrics?.cpuPercent > 0 ? 'text-green-400' : 'text-gray-600'
        }`}>
          {isConnected && metrics?.cpuPercent !== undefined ? `${metrics.cpuPercent.toFixed(1)}%` : '—'}
        </span>

        {/* RAM */}
        <span className="font-mono text-xs text-gray-400">
          {isConnected && metrics?.memoryBytes ? formatBytes(metrics.memoryBytes) : '—'}
        </span>

        {/* Host */}
        <span className="flex items-center gap-1 text-xs text-gray-400 truncate">
          <HardDrive className="w-3 h-3 shrink-0" />
          {host ? host.name : 'Local'}
        </span>

        {/* Actions */}
        <div className={`flex items-center gap-0.5 justify-end ${isMigrating || isHostOffline ? 'opacity-50 pointer-events-none' : ''}`}>
          {isConnected ? (
            <button onClick={() => onStop(container.id)} className="p-1 text-yellow-400 hover:text-yellow-300 hover:bg-yellow-900/30 transition-colors" title="Arreter">
              <Square className="w-3.5 h-3.5" />
            </button>
          ) : displayStatus !== 'deploying' ? (
            <button onClick={() => onStart(container.id)} className="p-1 text-green-400 hover:text-green-300 hover:bg-green-900/30 transition-colors" title="Demarrer">
              <Play className="w-3.5 h-3.5" />
            </button>
          ) : null}
          <button onClick={() => onTerminal(container)} disabled={isMigrating} className="p-1 text-emerald-400 hover:text-emerald-300 hover:bg-emerald-900/30 transition-colors" title="Terminal">
            <Terminal className="w-3.5 h-3.5" />
          </button>
          <button onClick={() => onMigrate(container)} disabled={isMigrating} className="p-1 text-gray-400 hover:text-blue-400 hover:bg-gray-700 transition-colors" title="Migrer">
            <ArrowRightLeft className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Mobile: card layout */}
      <div className="lg:hidden px-4 py-3 border-b border-gray-700/30 space-y-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className={`text-xs px-1.5 py-0.5 font-medium ${
              isDev ? 'bg-blue-100 text-blue-800' : 'bg-purple-100 text-purple-800'
            }`}>
              {isDev ? 'DEV' : 'PROD'}
            </span>
            <StatusBadge status={displayStatus} />
          </div>
          <div className={`flex items-center gap-1 ${isMigrating || isHostOffline ? 'opacity-50 pointer-events-none' : ''}`}>
            {isConnected ? (
              <button onClick={() => onStop(container.id)} className="p-1.5 text-yellow-400 hover:text-yellow-300" title="Arreter">
                <Square className="w-4 h-4" />
              </button>
            ) : displayStatus !== 'deploying' ? (
              <button onClick={() => onStart(container.id)} className="p-1.5 text-green-400 hover:text-green-300" title="Demarrer">
                <Play className="w-4 h-4" />
              </button>
            ) : null}
            <button onClick={() => onTerminal(container)} disabled={isMigrating} className="p-1.5 text-emerald-400" title="Terminal">
              <Terminal className="w-4 h-4" />
            </button>
            <button onClick={() => onMigrate(container)} disabled={isMigrating} className="p-1.5 text-gray-400" title="Migrer">
              <ArrowRightLeft className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* Access links + metrics */}
        <div className="flex items-center gap-2 flex-wrap">
          {prodAppUrl && (
            <a href={`https://${prodAppUrl}`} target="_blank" rel="noopener noreferrer"
              className="inline-flex items-center gap-0.5 px-1.5 py-0.5 text-xs text-green-400 bg-green-900/20 rounded">
              <ExternalLink className="w-3 h-3" /> APP
            </a>
          )}
          <span className="text-xs text-gray-500 ml-auto flex items-center gap-2">
            {isConnected && metrics?.cpuPercent !== undefined && (
              <span className={metrics.cpuPercent > 80 ? 'text-red-400' : metrics.cpuPercent > 50 ? 'text-yellow-400' : 'text-green-400'}>
                CPU {metrics.cpuPercent.toFixed(1)}%
              </span>
            )}
            {isConnected && metrics?.memoryBytes && (
              <span className="text-gray-400">RAM {formatBytes(metrics.memoryBytes)}</span>
            )}
          </span>
        </div>

        {/* Security + host info */}
        <div className="flex items-center gap-3 text-xs text-gray-500">
          <span className="flex items-center gap-1">
            <HardDrive className="w-3 h-3" />
            {host ? host.name : 'Local'}
          </span>
          <button
            onClick={() => onToggleSecurity(container.id, 'auth_required', !container.frontend?.auth_required)}
            className={`flex items-center gap-1 ${container.frontend?.auth_required ? 'text-purple-400' : 'text-purple-400 opacity-30'}`}
          >
            <Key className="w-3 h-3" /> Auth
          </button>
          <button
            onClick={() => onToggleSecurity(container.id, 'local_only', !container.frontend?.local_only)}
            className={`flex items-center gap-1 ${container.frontend?.local_only ? 'text-yellow-400' : 'text-yellow-400 opacity-30'}`}
          >
            <Shield className="w-3 h-3" /> Local
          </button>
        </div>
      </div>

      {/* Migration progress (sub-row) */}
      {isMigrating && MigrationProgress && (
        <MigrationProgress
          appId={container.id}
          migration={migration}
          onDismiss={onMigrationDismiss}
        />
      )}
    </div>
  );
}

export default ContainerCard;
