import { Database, ChevronRight, Table2 } from 'lucide-react';

export default function SitemapPanel({ apps, selectedAppId, selectedTableName, envFilter, onSelectTable }) {
  const filtered = (apps || []).filter(app => app.environment === envFilter);

  return (
    <div className="w-60 flex-shrink-0 border-r border-gray-700 flex flex-col bg-gray-800 overflow-y-auto">
      {filtered.map(app => {
        const isExpanded = app.appId === selectedAppId;
        const tables = app.tables || [];

        return (
          <div key={app.appId}>
            {/* App header */}
            <button
              className={`w-full flex items-center gap-2 px-3 py-2 text-left transition-colors ${
                isExpanded
                  ? 'bg-blue-600/20 text-white'
                  : 'text-gray-300 hover:bg-gray-700/50'
              }`}
              onClick={() => {
                // Click the first table if any, or just select the app
                if (tables.length > 0) {
                  onSelectTable(app.appId, tables[0].name);
                }
              }}
            >
              <Database className={`w-4 h-4 flex-shrink-0 ${isExpanded ? 'text-blue-400' : ''}`} />
              <span className="text-sm font-medium truncate flex-1">{app.slug}</span>
              <span className="text-xs text-gray-500">{tables.length}</span>
              <ChevronRight
                className={`w-4 h-4 flex-shrink-0 text-gray-500 transition-transform duration-150 ${
                  isExpanded ? 'rotate-90' : ''
                }`}
              />
            </button>

            {/* Tables list */}
            {isExpanded && tables.length > 0 && (
              <div className="bg-gray-900/30">
                {tables.map(table => {
                  const isActive = selectedTableName === table.name;
                  return (
                    <button
                      key={table.name}
                      className={`w-full flex items-center gap-2 px-4 pl-8 py-1.5 text-left text-sm transition-colors ${
                        isActive
                          ? 'bg-blue-600/90 text-white'
                          : 'text-gray-400 hover:bg-gray-700/50 hover:text-gray-200'
                      }`}
                      onClick={() => onSelectTable(app.appId, table.name)}
                    >
                      <Table2 className={`w-3.5 h-3.5 flex-shrink-0 ${isActive ? 'text-white' : 'text-green-500'}`} />
                      <span className="font-mono truncate flex-1">{table.name}</span>
                      <span className="text-xs flex-shrink-0">{table.rowsCount ?? ''}</span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        );
      })}

      {filtered.length === 0 && (
        <div className="px-4 py-8 text-center text-sm text-gray-500">
          Aucune application
        </div>
      )}
    </div>
  );
}
