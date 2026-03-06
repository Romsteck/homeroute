import { useState } from 'react';
import { ChevronDown, ChevronUp, ArrowUp, CheckCircle, AlertTriangle, Loader2 } from 'lucide-react';
import Button from './Button';

function VersionRow({ label, installed, latest, category, targetId, onUpgrade, upgrading }) {
  if (!installed && !latest) return null;

  const isUpToDate = installed && latest && installed === latest;
  const hasUpdate = installed && latest && installed !== latest;

  return (
    <div className="flex items-center justify-between py-1.5 border-b border-gray-700/30 last:border-0">
      <div className="flex items-center gap-2 min-w-0">
        <span className="text-gray-400 text-sm w-28 shrink-0">{label}</span>
        <span className="font-mono text-sm text-gray-300 truncate">
          {installed || '—'}
        </span>
        {hasUpdate && (
          <>
            <span className="text-gray-500">→</span>
            <span className="font-mono text-sm text-orange-400">{latest}</span>
          </>
        )}
      </div>
      <div className="flex items-center gap-2 shrink-0 ml-2">
        {isUpToDate && (
          <span className="flex items-center gap-1 text-green-400 text-xs">
            <CheckCircle className="w-3.5 h-3.5" /> À jour
          </span>
        )}
        {hasUpdate && !upgrading && (
          <Button
            variant="warning"
            size="sm"
            onClick={() => onUpgrade(targetId, category)}
          >
            <ArrowUp className="w-3 h-3 mr-1" />
            Mettre à jour
          </Button>
        )}
        {upgrading && (
          <span className="flex items-center gap-1 text-blue-400 text-xs">
            <Loader2 className="w-3.5 h-3.5 animate-spin" /> En cours...
          </span>
        )}
        {!installed && latest && (
          <span className="text-gray-500 text-xs">Non installé</span>
        )}
      </div>
    </div>
  );
}

export default function TargetUpdateCard({ target, upgradeState = {}, onUpgrade }) {
  const [expanded, setExpanded] = useState(false);
  const isOnline = target.online;
  const isUpgrading = (cat) => upgradeState[cat]?.running;

  const totalUpdates = (target.os_upgradable || 0);
  const hasSecurity = (target.os_security || 0) > 0;
  const isDev = target.environment === 'development';

  return (
    <div className={`bg-gray-800 border border-gray-700 ${!isOnline ? 'opacity-50' : ''}`}>
      {/* Header */}
      <div
        className="px-4 py-3 flex items-center justify-between cursor-pointer hover:bg-gray-700/30"
        onClick={() => setExpanded(!expanded)}
      >
        <div className="flex items-center gap-3">
          <div className={`w-2 h-2 ${isOnline ? 'bg-green-400' : 'bg-gray-500'}`} />
          <span className="font-medium text-gray-100">{target.name}</span>
          {target.environment && (
            <span className={`text-xs px-1.5 py-0.5 ${isDev ? 'bg-blue-500/20 text-blue-400' : 'bg-purple-500/20 text-purple-400'}`}>
              {isDev ? 'DEV' : 'PROD'}
            </span>
          )}
        </div>
        <div className="flex items-center gap-3">
          {totalUpdates > 0 && (
            <span className="flex items-center gap-1 text-orange-400 text-xs">
              <ArrowUp className="w-3 h-3" /> {totalUpdates} MàJ
              {hasSecurity && <span className="text-red-400">({target.os_security} sécu)</span>}
            </span>
          )}
          {totalUpdates === 0 && isOnline && (
            <span className="text-green-400 text-xs flex items-center gap-1">
              <CheckCircle className="w-3 h-3" /> OS à jour
            </span>
          )}
          {target.scan_error && (
            <span className="text-red-400 text-xs flex items-center gap-1">
              <AlertTriangle className="w-3 h-3" /> Erreur
            </span>
          )}
          {expanded ? <ChevronUp className="w-4 h-4 text-gray-400" /> : <ChevronDown className="w-4 h-4 text-gray-400" />}
        </div>
      </div>

      {/* Expanded content */}
      {expanded && isOnline && (
        <div className="px-4 pb-3 border-t border-gray-700">
          <div className="pt-2">
            {/* OS updates */}
            <VersionRow
              label="OS (APT)"
              installed={totalUpdates > 0 ? `${totalUpdates} paquets` : 'À jour'}
              latest={totalUpdates > 0 ? 'Disponible' : 'À jour'}
              category="apt"
              targetId={target.id}
              onUpgrade={onUpgrade}
              upgrading={isUpgrading('apt')}
            />

            {/* Agent version */}
            {target.agent_version && (
              <VersionRow
                label="Agent"
                installed={target.agent_version}
                latest={target.agent_version_latest}
                category="hr_agent"
                targetId={target.id}
                onUpgrade={onUpgrade}
                upgrading={isUpgrading('hr_agent')}
              />
            )}

            {/* DEV-only components */}
            {isDev && (
              <>
                <VersionRow
                  label="Claude Code"
                  installed={target.claude_cli_installed}
                  latest={target.claude_cli_latest}
                  category="claude_cli"
                  targetId={target.id}
                  onUpgrade={onUpgrade}
                  upgrading={isUpgrading('claude_cli')}
                />
                <VersionRow
                  label="code-server"
                  installed={target.code_server_installed}
                  latest={target.code_server_latest}
                  category="code_server"
                  targetId={target.id}
                  onUpgrade={onUpgrade}
                  upgrading={isUpgrading('code_server')}
                />
                <VersionRow
                  label="Extension"
                  installed={target.claude_ext_installed}
                  latest={target.claude_ext_latest}
                  category="claude_ext"
                  targetId={target.id}
                  onUpgrade={onUpgrade}
                  upgrading={isUpgrading('claude_ext')}
                />
              </>
            )}

            {/* Upgrade output */}
            {Object.values(upgradeState).some(s => s?.output?.length > 0) && (
              <div className="mt-2 bg-gray-900 p-2 font-mono text-xs max-h-32 overflow-y-auto">
                {Object.values(upgradeState)
                  .flatMap(s => s?.output || [])
                  .slice(-50)
                  .map((line, i) => (
                    <div key={i} className="text-gray-400">{line}</div>
                  ))}
              </div>
            )}

            {/* Scan error */}
            {target.scan_error && (
              <div className="mt-2 text-red-400 text-xs">
                <AlertTriangle className="w-3 h-3 inline mr-1" />
                {target.scan_error}
              </div>
            )}

            {/* Last scanned */}
            {target.scanned_at && (
              <div className="mt-2 text-gray-500 text-xs">
                Scanné : {new Date(target.scanned_at).toLocaleString('fr-FR')}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
