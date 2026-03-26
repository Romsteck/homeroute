import { Link } from 'react-router-dom';
import {
  ExternalLink,
  Play,
  Square,
  Terminal,
  ArrowRightLeft,
  Key,
  Shield,
  Pencil,
  Trash2,
  HardDrive,
  BookOpen,
} from 'lucide-react';

function formatBytes(bytes) {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(0)) + ' ' + sizes[i];
}

const STATUS_DOT = {
  connected: 'bg-green-400',
  deploying: 'bg-blue-400 animate-pulse',
  pending: 'bg-gray-400',
  running: 'bg-gray-400',
  stopped: 'bg-gray-500',
  disconnected: 'bg-red-400',
  error: 'bg-red-400',
};

const STATUS_LABEL = {
  connected: 'Connecte',
  deploying: 'Deploiement',
  pending: 'En attente',
  running: 'En attente',
  stopped: 'Arrete',
  disconnected: 'Deconnecte',
  error: 'Erreur',
};

function MetricBar({ value, max, thresholds }) {
  const pct = max > 0 ? Math.min((value / max) * 100, 100) : 0;
  const color = pct > thresholds[1] ? 'bg-red-400' : pct > thresholds[0] ? 'bg-yellow-400' : 'bg-green-400';
  return (
    <div className="w-full bg-gray-700 h-1.5 rounded-full overflow-hidden">
      <div className={`h-1.5 rounded-full transition-all duration-500 ${color}`} style={{ width: `${pct}%` }} />
    </div>
  );
}

function ApplicationCard({
  container,
  baseDomain,
  metrics,
  migration,
  hosts,
  onStart,
  onStop,
  onTerminal,
  onEdit,
  onDelete,
  onToggleSecurity,
  onMigrate,
  onVolumes,
  onMigrationDismiss,
  MigrationProgress,
}) {
  const displayStatus = container.agent_status || container.status;
  const isMigrating = !!migration;
  const isConnected = displayStatus === 'connected';
  const dotClass = STATUS_DOT[displayStatus] || STATUS_DOT.disconnected;
  const statusLabel = STATUS_LABEL[displayStatus] || 'Inconnu';
  const host = container.host_id && container.host_id !== 'local'
    ? hosts?.find(h => h.id === container.host_id)
    : null;
  const isHostOffline = host && host.status !== 'online';
  const appUrl = baseDomain ? `${container.slug}.${baseDomain}` : null;

  return (
    <div className={`group bg-gray-800 border border-gray-700 rounded-lg hover:border-gray-600 transition-colors ${isHostOffline ? 'opacity-60' : ''}`}>
      {/* Header */}
      <div className="px-4 pt-3 pb-2">
        <div className="flex items-center gap-2 min-w-0">
          <span className={`w-2.5 h-2.5 rounded-full shrink-0 ${dotClass}`} />
          <span className="font-semibold text-sm truncate">{container.name || container.slug}</span>
        </div>
        <div className="flex items-center gap-2 mt-0.5 ml-[18px]">
          <span className="text-xs text-gray-500 font-mono">{container.slug}</span>
          <span className={`text-[10px] ${
            displayStatus === 'connected' ? 'text-green-400' :
            displayStatus === 'deploying' ? 'text-blue-400' :
            displayStatus === 'error' || displayStatus === 'disconnected' ? 'text-red-400' :
            'text-gray-500'
          }`}>
            {statusLabel}
          </span>
        </div>
      </div>

      {/* Metrics */}
      {isConnected && metrics && (
        <div className="px-4 py-2 space-y-1.5">
          <div className="flex items-center gap-2">
            <span className="text-[10px] text-gray-500 w-7">CPU</span>
            <MetricBar value={metrics.cpuPercent || 0} max={100} thresholds={[50, 80]} />
            <span className="text-[10px] font-mono text-gray-400 w-10 text-right">
              {metrics.cpuPercent !== undefined ? `${metrics.cpuPercent.toFixed(0)}%` : '—'}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-[10px] text-gray-500 w-7">RAM</span>
            <MetricBar value={metrics.memoryBytes || 0} max={512 * 1024 * 1024} thresholds={[50, 80]} />
            <span className="text-[10px] font-mono text-gray-400 w-10 text-right">
              {metrics.memoryBytes ? formatBytes(metrics.memoryBytes) : '—'}
            </span>
          </div>
        </div>
      )}

      {/* Meta row */}
      <div className="px-4 py-1.5 flex items-center gap-2 text-xs text-gray-500">
        <div className="flex items-center gap-1.5">
          <button
            onClick={() => onToggleSecurity(container.id, 'auth_required', !container.frontend?.auth_required)}
            className={`transition-colors ${
              container.frontend?.auth_required
                ? 'text-purple-400 hover:text-purple-300'
                : 'text-purple-400 opacity-30 hover:opacity-60'
            }`}
            title={container.frontend?.auth_required ? 'Auth requis (cliquer pour desactiver)' : 'Auth non requis (cliquer pour activer)'}
          >
            <Key className="w-3 h-3" />
          </button>
          <button
            onClick={() => onToggleSecurity(container.id, 'local_only', !container.frontend?.local_only)}
            className={`transition-colors ${
              container.frontend?.local_only
                ? 'text-yellow-400 hover:text-yellow-300'
                : 'text-yellow-400 opacity-30 hover:opacity-60'
            }`}
            title={container.frontend?.local_only ? 'Local uniquement (cliquer pour desactiver)' : 'Acces externe (cliquer pour restreindre au local)'}
          >
            <Shield className="w-3 h-3" />
          </button>
        </div>
        <span className="ml-auto flex items-center gap-1 truncate">
          <HardDrive className="w-3 h-3 shrink-0" />
          {host ? host.name : 'Local'}
        </span>
      </div>

      {/* Migration progress */}
      {isMigrating && MigrationProgress && (
        <MigrationProgress
          appId={container.id}
          migration={migration}
          onDismiss={() => onMigrationDismiss(container.id)}
        />
      )}

      {/* Actions bar */}
      <div className={`px-3 py-2 border-t border-gray-700/50 flex items-center gap-1 ${isMigrating || isHostOffline ? 'opacity-50 pointer-events-none' : ''}`}>
        {appUrl && (
          <a
            href={`https://${appUrl}`}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-0.5 px-1.5 py-1 text-xs text-green-400 hover:text-green-300 hover:bg-green-900/20 rounded transition-colors"
            title="Ouvrir l'application"
          >
            <ExternalLink className="w-3.5 h-3.5" />
            <span className="hidden sm:inline">APP</span>
          </a>
        )}

        {isConnected ? (
          <button onClick={() => onStop(container.id)} className="p-1 text-yellow-400 hover:text-yellow-300 hover:bg-yellow-900/30 rounded transition-colors" title="Arreter">
            <Square className="w-3.5 h-3.5" />
          </button>
        ) : displayStatus !== 'deploying' ? (
          <button onClick={() => onStart(container.id)} className="p-1 text-green-400 hover:text-green-300 hover:bg-green-900/30 rounded transition-colors" title="Demarrer">
            <Play className="w-3.5 h-3.5" />
          </button>
        ) : null}

        <button onClick={() => onTerminal(container)} disabled={isMigrating} className="p-1 text-emerald-400 hover:text-emerald-300 hover:bg-emerald-900/30 rounded transition-colors" title="Terminal">
          <Terminal className="w-3.5 h-3.5" />
        </button>
        <button onClick={() => onMigrate(container)} disabled={isMigrating} className="p-1 text-gray-400 hover:text-blue-400 hover:bg-gray-700 rounded transition-colors" title="Migrer">
          <ArrowRightLeft className="w-3.5 h-3.5" />
        </button>
        <button onClick={() => onVolumes(container)} className="p-1 text-gray-400 hover:text-purple-400 hover:bg-gray-700 rounded transition-colors" title="Volumes">
          <HardDrive className="w-3.5 h-3.5" />
        </button>
        <Link to={`/docs/${container.slug}`} className="p-1 text-gray-400 hover:text-cyan-400 hover:bg-gray-700 rounded transition-colors" title="Documentation">
          <BookOpen className="w-3.5 h-3.5" />
        </Link>

        {/* Edit/Delete visible on hover */}
        <div className="ml-auto flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
          <button
            onClick={() => onEdit(container)}
            className="p-1 text-gray-500 hover:text-blue-400 hover:bg-gray-700 rounded transition-colors"
            title="Modifier"
          >
            <Pencil className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={() => onDelete(container)}
            className="p-1 text-gray-500 hover:text-red-400 hover:bg-gray-700 rounded transition-colors"
            title="Supprimer"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>
    </div>
  );
}

function ApplicationRow({
  container,
  baseDomain,
  metrics,
  hosts,
  onStart,
  onStop,
  onTerminal,
  onEdit,
  onDelete,
  onToggleSecurity,
  onMigrate,
  onVolumes,
}) {
  const displayStatus = container.agent_status || container.status;
  const isConnected = displayStatus === 'connected';
  const dotClass = STATUS_DOT[displayStatus] || STATUS_DOT.disconnected;
  const statusLabel = STATUS_LABEL[displayStatus] || 'Inconnu';
  const host = container.host_id && container.host_id !== 'local'
    ? hosts?.find(h => h.id === container.host_id)
    : null;
  const isHostOffline = host && host.status !== 'online';
  const appUrl = baseDomain ? `${container.slug}.${baseDomain}` : null;

  return (
    <tr className={`border-b border-gray-700/50 hover:bg-gray-800/50 transition-colors group ${isHostOffline ? 'opacity-60' : ''}`}>
      {/* Name */}
      <td className="px-3 py-2.5">
        <div className="flex items-center gap-2 min-w-0">
          <span className={`w-2 h-2 rounded-full shrink-0 ${dotClass}`} />
          <span className="font-medium text-sm truncate">{container.name || container.slug}</span>
          <span className="text-xs text-gray-500 font-mono hidden lg:inline">{container.slug}</span>
        </div>
      </td>

      {/* Status */}
      <td className="px-3 py-2.5">
        <span className={`text-xs ${
          displayStatus === 'connected' ? 'text-green-400' :
          displayStatus === 'deploying' ? 'text-blue-400' :
          displayStatus === 'error' || displayStatus === 'disconnected' ? 'text-red-400' :
          'text-gray-500'
        }`}>
          {statusLabel}
        </span>
      </td>

      {/* IP */}
      <td className="px-3 py-2.5 hidden lg:table-cell">
        <span className="text-xs font-mono text-gray-400">{container.ipv4_address || '—'}</span>
      </td>

      {/* Host */}
      <td className="px-3 py-2.5 hidden xl:table-cell">
        <span className="text-xs text-gray-400 flex items-center gap-1">
          <HardDrive className="w-3 h-3 shrink-0" />
          {host ? host.name : 'Local'}
        </span>
      </td>

      {/* CPU */}
      <td className="px-3 py-2.5 hidden xl:table-cell">
        {isConnected && metrics ? (
          <div className="flex items-center gap-1.5 min-w-[80px]">
            <div className="flex-1">
              <MetricBar value={metrics.cpuPercent || 0} max={100} thresholds={[50, 80]} />
            </div>
            <span className="text-[10px] font-mono text-gray-400 w-10 text-right whitespace-nowrap">
              {metrics.cpuPercent !== undefined ? `${metrics.cpuPercent.toFixed(0)}%` : '—'}
            </span>
          </div>
        ) : (
          <span className="text-xs text-gray-600">—</span>
        )}
      </td>

      {/* RAM */}
      <td className="px-3 py-2.5 hidden xl:table-cell">
        {isConnected && metrics ? (
          <div className="flex items-center gap-1.5 min-w-[80px]">
            <div className="flex-1">
              <MetricBar value={metrics.memoryBytes || 0} max={512 * 1024 * 1024} thresholds={[50, 80]} />
            </div>
            <span className="text-[10px] font-mono text-gray-400 w-14 text-right whitespace-nowrap">
              {metrics.memoryBytes ? formatBytes(metrics.memoryBytes) : '—'}
            </span>
          </div>
        ) : (
          <span className="text-xs text-gray-600">—</span>
        )}
      </td>

      {/* Security */}
      <td className="px-3 py-2.5 hidden lg:table-cell">
        <div className="flex items-center gap-1.5">
          <button
            onClick={() => onToggleSecurity(container.id, 'auth_required', !container.frontend?.auth_required)}
            className={`transition-colors ${
              container.frontend?.auth_required
                ? 'text-purple-400 hover:text-purple-300'
                : 'text-purple-400 opacity-30 hover:opacity-60'
            }`}
            title={container.frontend?.auth_required ? 'Auth requis' : 'Auth non requis'}
          >
            <Key className="w-3 h-3" />
          </button>
          <button
            onClick={() => onToggleSecurity(container.id, 'local_only', !container.frontend?.local_only)}
            className={`transition-colors ${
              container.frontend?.local_only
                ? 'text-yellow-400 hover:text-yellow-300'
                : 'text-yellow-400 opacity-30 hover:opacity-60'
            }`}
            title={container.frontend?.local_only ? 'Local uniquement' : 'Acces externe'}
          >
            <Shield className="w-3 h-3" />
          </button>
        </div>
      </td>

      {/* URL */}
      <td className="px-3 py-2.5 hidden lg:table-cell">
        {appUrl ? (
          <a
            href={`https://${appUrl}`}
            target="_blank"
            rel="noopener noreferrer"
            className="text-xs text-green-400 hover:text-green-300 hover:underline truncate block max-w-[200px]"
          >
            {appUrl}
          </a>
        ) : (
          <span className="text-xs text-gray-600">—</span>
        )}
      </td>

      {/* Actions */}
      <td className="px-3 py-2.5">
        <div className={`flex items-center gap-0.5 ${isHostOffline ? 'opacity-50 pointer-events-none' : ''}`}>
          {appUrl && (
            <a
              href={`https://${appUrl}`}
              target="_blank"
              rel="noopener noreferrer"
              className="p-1 text-green-400 hover:text-green-300 hover:bg-green-900/20 rounded transition-colors"
              title="Ouvrir"
            >
              <ExternalLink className="w-3.5 h-3.5" />
            </a>
          )}
          {isConnected ? (
            <button onClick={() => onStop(container.id)} className="p-1 text-yellow-400 hover:text-yellow-300 hover:bg-yellow-900/30 rounded transition-colors" title="Arreter">
              <Square className="w-3.5 h-3.5" />
            </button>
          ) : displayStatus !== 'deploying' ? (
            <button onClick={() => onStart(container.id)} className="p-1 text-green-400 hover:text-green-300 hover:bg-green-900/30 rounded transition-colors" title="Demarrer">
              <Play className="w-3.5 h-3.5" />
            </button>
          ) : null}
          <button onClick={() => onTerminal(container)} className="p-1 text-emerald-400 hover:text-emerald-300 hover:bg-emerald-900/30 rounded transition-colors" title="Terminal">
            <Terminal className="w-3.5 h-3.5" />
          </button>
          <button onClick={() => onMigrate(container)} className="p-1 text-gray-400 hover:text-blue-400 hover:bg-gray-700 rounded transition-colors" title="Migrer">
            <ArrowRightLeft className="w-3.5 h-3.5" />
          </button>
          <button onClick={() => onVolumes(container)} className="p-1 text-gray-400 hover:text-purple-400 hover:bg-gray-700 rounded transition-colors" title="Volumes">
            <HardDrive className="w-3.5 h-3.5" />
          </button>
          <Link to={`/docs/${container.slug}`} className="p-1 text-gray-400 hover:text-cyan-400 hover:bg-gray-700 rounded transition-colors" title="Documentation">
            <BookOpen className="w-3.5 h-3.5" />
          </Link>
          <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
            <button onClick={() => onEdit(container)} className="p-1 text-gray-500 hover:text-blue-400 hover:bg-gray-700 rounded transition-colors" title="Modifier">
              <Pencil className="w-3.5 h-3.5" />
            </button>
            <button onClick={() => onDelete(container)} className="p-1 text-gray-500 hover:text-red-400 hover:bg-gray-700 rounded transition-colors" title="Supprimer">
              <Trash2 className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </td>
    </tr>
  );
}

export { ApplicationRow };
export default ApplicationCard;
