import { Container, Rocket, Plus } from 'lucide-react';
import ContainerCard from './ContainerCard';
import Button from './Button';

function AppGroupCard({
  group,
  baseDomain,
  appMetrics,
  migrations,
  hosts,
  onStart,
  onStop,
  onTerminal,
  onEdit,
  onMigrate,
  onDelete,
  onMigrationDismiss,
  onDeploy,
  onCreatePaired,
  MigrationProgress,
}) {
  const { slug, name, dev, prod } = group;
  const devStatus = dev ? (dev.agent_status || dev.status) : null;
  const canDeploy = dev && prod && devStatus === 'connected';

  return (
    <div className="bg-gray-800 border-b border-gray-700">
      {/* Group header */}
      <div className="px-4 py-2 border-b border-gray-700 bg-gray-800/60 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Container className="w-4 h-4 text-blue-400" />
          <span className="font-semibold text-sm">{name || slug}</span>
          <span className="text-xs text-gray-500 font-mono">{slug}</span>
        </div>
        <div className="flex items-center gap-2">
          {canDeploy && (
            <Button
              onClick={() => onDeploy(dev, prod)}
              variant="primary"
              className="!px-3 !py-1 text-xs"
            >
              <Rocket className="w-3.5 h-3.5" />
              Deploy
            </Button>
          )}
        </div>
      </div>

      {/* Container rows */}
      {dev ? (
        <ContainerCard
          container={dev}
          baseDomain={baseDomain}
          metrics={appMetrics[dev.id]}
          migration={migrations[dev.id]}
          hosts={hosts}
          isHostOffline={dev.host_id && dev.host_id !== 'local' && hosts.find(h => h.id === dev.host_id)?.status !== 'online'}
          onStart={onStart}
          onStop={onStop}
          onTerminal={onTerminal}
          onEdit={onEdit}
          onMigrate={onMigrate}
          onDelete={onDelete}
          onMigrationDismiss={() => onMigrationDismiss(dev.id)}
          MigrationProgress={MigrationProgress}
        />
      ) : (
        <div className="flex items-center gap-2 px-4 py-1.5 border-b border-gray-700/30">
          <span className="text-xs px-1.5 py-0.5 font-medium bg-blue-100 text-blue-800">DEV</span>
          <button
            onClick={() => onCreatePaired(slug, name, 'development', prod?.id)}
            className="flex items-center gap-1 text-xs text-gray-500 hover:text-blue-400 transition-colors"
          >
            <Plus className="w-3.5 h-3.5" />
            Creer conteneur DEV
          </button>
        </div>
      )}

      {prod ? (
        <ContainerCard
          container={prod}
          baseDomain={baseDomain}
          metrics={appMetrics[prod.id]}
          migration={migrations[prod.id]}
          hosts={hosts}
          isHostOffline={prod.host_id && prod.host_id !== 'local' && hosts.find(h => h.id === prod.host_id)?.status !== 'online'}
          onStart={onStart}
          onStop={onStop}
          onTerminal={onTerminal}
          onEdit={onEdit}
          onMigrate={onMigrate}
          onDelete={onDelete}
          onMigrationDismiss={() => onMigrationDismiss(prod.id)}
          MigrationProgress={MigrationProgress}
        />
      ) : (
        <div className="flex items-center gap-2 px-4 py-1.5 border-b border-gray-700/30">
          <span className="text-xs px-1.5 py-0.5 font-medium bg-purple-100 text-purple-800">PROD</span>
          <button
            onClick={() => onCreatePaired(slug, name, 'production', dev?.id)}
            className="flex items-center gap-1 text-xs text-gray-500 hover:text-purple-400 transition-colors"
          >
            <Plus className="w-3.5 h-3.5" />
            Creer conteneur PROD
          </button>
        </div>
      )}
    </div>
  );
}

export default AppGroupCard;
