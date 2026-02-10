import {
  Wifi,
  WifiOff,
  Clock,
  Loader2,
  Play,
  Square,
  Terminal,
  Pencil,
  ArrowRightLeft,
  Trash2,
  Code2,
  Key,
  Shield,
  AlertTriangle,
  HardDrive,
  ExternalLink,
} from 'lucide-react';

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
  onEdit,
  onMigrate,
  onDelete,
  onMigrationDismiss,
  MigrationProgress,
}) {
  const displayStatus = container.agent_status || container.status;
  const isDeploying = displayStatus === 'deploying' || container.status === 'deploying';
  const isMigrating = !!migration;
  const isDev = container.environment !== 'production';
  const host = container.host_id && container.host_id !== 'local'
    ? hosts?.find(h => h.id === container.host_id)
    : null;

  const appUrl = baseDomain
    ? isDev
      ? `dev.${container.slug}.${baseDomain}`
      : `${container.slug}.${baseDomain}`
    : null;

  const ideUrl = baseDomain && isDev && container.code_server_enabled
    ? `code.${container.slug}.${baseDomain}`
    : null;

  return (
    <div className={isHostOffline ? 'opacity-60' : ''}>
      {/* Main row */}
      <div className="flex items-center gap-3 px-4 py-1.5 border-b border-gray-700/30">
        {/* Environment badge */}
        <span className={`text-xs px-1.5 py-0.5 font-medium shrink-0 ${
          isDev ? 'bg-blue-100 text-blue-800' : 'bg-purple-100 text-purple-800'
        }`}>
          {isDev ? 'DEV' : 'PROD'}
        </span>

        {/* Status */}
        <StatusBadge status={displayStatus} />

        {/* URL + links */}
        <div className="flex items-center gap-2 min-w-0 flex-1">
          {appUrl && (
            <a
              href={`https://${appUrl}`}
              target="_blank"
              rel="noopener noreferrer"
              className="font-mono text-xs text-gray-400 hover:text-blue-400 truncate flex items-center gap-1"
            >
              {appUrl}
              <ExternalLink className="w-3 h-3 shrink-0" />
            </a>
          )}
          {container.frontend?.auth_required && <Key className="w-3 h-3 text-purple-400 shrink-0" />}
          {container.frontend?.local_only && <Shield className="w-3 h-3 text-yellow-400 shrink-0" />}
          {ideUrl && (
            <a
              href={`https://${ideUrl}`}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1 px-1.5 py-0.5 text-xs text-cyan-400 hover:text-cyan-300 bg-cyan-900/20 shrink-0"
            >
              <Code2 className="w-3 h-3" />
              IDE
            </a>
          )}
        </div>

        {/* Deploying message */}
        {isDeploying && container._deployMessage && (
          <span className="text-xs text-gray-500 truncate shrink-0 max-w-[150px]">{container._deployMessage}</span>
        )}

        {/* Metrics */}
        {!isMigrating && displayStatus === 'connected' && metrics && (
          <div className="flex items-center gap-3 text-xs shrink-0">
            <span className={`font-mono ${
              metrics.cpuPercent > 80 ? 'text-red-400' :
              metrics.cpuPercent > 50 ? 'text-yellow-400' :
              metrics.cpuPercent > 0 ? 'text-green-400' : 'text-gray-600'
            }`}>
              CPU {metrics.cpuPercent !== undefined ? `${metrics.cpuPercent.toFixed(1)}%` : '-'}
            </span>
            <span className="font-mono text-gray-400">
              RAM {metrics.memoryBytes ? formatBytes(metrics.memoryBytes) : '-'}
            </span>
          </div>
        )}

        {/* Host badge */}
        <div className="shrink-0">
          {host ? (
            <span className="flex items-center gap-1 text-xs bg-gray-100 text-gray-700 px-1.5 py-0.5">
              <HardDrive className="w-3 h-3" />
              {host.name}
            </span>
          ) : container.host_id === 'local' ? (
            <span className="flex items-center gap-1 text-xs bg-gray-100 text-gray-700 px-1.5 py-0.5">
              <HardDrive className="w-3 h-3" />
              Local
            </span>
          ) : null}
        </div>

        {/* Actions */}
        <div className={`flex items-center gap-0.5 shrink-0 ${isMigrating || isHostOffline ? 'opacity-50 pointer-events-none' : ''}`}>
          {displayStatus === 'connected' ? (
            <button
              onClick={() => onStop(container.id)}
              className="p-1 text-yellow-400 hover:text-yellow-300 hover:bg-yellow-900/30 transition-colors"
              title="Arreter"
            >
              <Square className="w-3.5 h-3.5" />
            </button>
          ) : displayStatus !== 'deploying' ? (
            <button
              onClick={() => onStart(container.id)}
              className="p-1 text-green-400 hover:text-green-300 hover:bg-green-900/30 transition-colors"
              title="Demarrer"
            >
              <Play className="w-3.5 h-3.5" />
            </button>
          ) : null}
          <button
            onClick={() => onTerminal(container)}
            disabled={isMigrating}
            className="p-1 text-emerald-400 hover:text-emerald-300 hover:bg-emerald-900/30 transition-colors"
            title="Terminal"
          >
            <Terminal className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={() => onEdit(container)}
            disabled={isMigrating}
            className="p-1 text-blue-400 hover:text-blue-300 hover:bg-blue-900/30 transition-colors"
            title="Modifier"
          >
            <Pencil className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={() => onMigrate(container)}
            disabled={isMigrating}
            className="p-1 text-gray-400 hover:text-blue-400 hover:bg-gray-700 transition-colors"
            title="Migrer"
          >
            <ArrowRightLeft className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={() => onDelete(container.id, container.name)}
            disabled={isMigrating}
            className="p-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 transition-colors"
            title="Supprimer"
          >
            <Trash2 className="w-3.5 h-3.5" />
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
