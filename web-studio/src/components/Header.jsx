import AppStatusDropdown from './AppStatusDropdown';

export default function Header({
  appName,
  activeTab,
  onTabChange,
  connected,
  authStatus,
  onOpenAuthDialog,
  apps,
  onStartApp,
  onStopApp,
  onRestartApp,
  onStartAll,
  onStopAll,
  onFetchLogs,
}) {
  const tabs = [
    { id: 'studio', label: 'Studio', icon: StudioIcon },
    { id: 'preview', label: 'Preview', icon: PreviewIcon },
    { id: 'code', label: 'Code Server', icon: CodeIcon },
    { id: 'files', label: 'Files', icon: FilesIcon },
    { id: 'docs', label: 'Docs', icon: DocsIcon },
  ];

  return (
    <header className="h-12 bg-gray-900/80 backdrop-blur border-b border-gray-800 flex items-center justify-between px-4 shrink-0">
      {/* Left: title */}
      <div className="flex items-center gap-2">
        <span className="text-sm text-gray-400 font-medium">
          Studio <span className="text-gray-600">&mdash;</span> <span className="text-gray-300">{appName}</span>
        </span>
      </div>

      {/* Center: tabs */}
      <div className="flex items-center gap-1">
        {tabs.map((tab) => {
          const isActive = activeTab === tab.id;
          const Icon = tab.icon;
          return (
            <button
              key={tab.id}
              onClick={() => onTabChange(tab.id)}
              className={`px-3 py-1.5 rounded-lg text-sm font-medium transition-colors flex items-center gap-2 ${
                isActive
                  ? 'bg-indigo-600/15 text-indigo-400'
                  : 'text-gray-500 hover:text-gray-300 hover:bg-gray-800/50'
              }`}
            >
              <Icon className="w-4 h-4" />
              {tab.label}
            </button>
          );
        })}
      </div>

      {/* Right: apps + auth + connection status */}
      <div className="flex items-center gap-3">
        {apps && apps.length > 0 && (
          <AppStatusDropdown
            apps={apps}
            onStart={onStartApp}
            onStop={onStopApp}
            onRestart={onRestartApp}
            onStartAll={onStartAll}
            onStopAll={onStopAll}
            onFetchLogs={onFetchLogs}
          />
        )}
        <div className="h-4 w-px bg-gray-800" />
        {authStatus?.authenticated ? (
          <button
            onClick={onOpenAuthDialog}
            className="flex items-center gap-1.5 px-2 py-1 rounded-md text-xs text-green-400 hover:bg-green-500/10 transition-colors"
          >
            <span className="inline-block w-1.5 h-1.5 rounded-full bg-green-500" />
            Claude Linked
          </button>
        ) : (
          <button
            onClick={onOpenAuthDialog}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium text-amber-400 bg-amber-500/10 hover:bg-amber-500/20 transition-colors"
          >
            Link Claude Account
          </button>
        )}
        <div className="h-4 w-px bg-gray-800" />
        <div className="flex items-center gap-2">
          <span className={`inline-block w-2 h-2 rounded-full ${connected ? 'bg-green-500' : 'bg-red-500'}`} />
          <span className={`text-xs ${connected ? 'text-gray-500' : 'text-red-400'}`}>
            {connected ? 'Connected' : 'Disconnected'}
          </span>
        </div>
      </div>
    </header>
  );
}

function StudioIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M6.75 7.5l3 2.25-3 2.25m4.5 0h3M5.25 20.25h13.5A2.25 2.25 0 0021 18V6a2.25 2.25 0 00-2.25-2.25H5.25A2.25 2.25 0 003 6v12a2.25 2.25 0 002.25 2.25z" />
    </svg>
  );
}

function PreviewIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z" />
      <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
    </svg>
  );
}

function FilesIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
    </svg>
  );
}

function CodeIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M17.25 6.75L22.5 12l-5.25 5.25m-10.5 0L1.5 12l5.25-5.25m7.5-3l-4.5 16.5" />
    </svg>
  );
}

function DocsIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M12 6.042A8.967 8.967 0 006 3.75c-1.052 0-2.062.18-3 .512v14.25A8.987 8.987 0 016 18c2.305 0 4.408.867 6 2.292m0-14.25a8.966 8.966 0 016-2.292c1.052 0 2.062.18 3 .512v14.25A8.987 8.987 0 0018 18a8.967 8.967 0 00-6 2.292m0-14.25v14.25" />
    </svg>
  );
}
