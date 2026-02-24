import SessionPicker from './SessionPicker';

export default function Header({
  appName,
  activeTab,
  onTabChange,
  connected,
  onToggleActivity,
  activityPanelOpen,
  activityCount,
  sessions,
  currentSessionId,
  onSelectSession,
  onNewSession,
  onDeleteSession,
}) {
  const tabs = [
    { id: 'agent', label: 'Agent', icon: AgentIcon },
    { id: 'preview', label: 'Preview', icon: PreviewIcon },
    { id: 'files', label: 'Files', icon: FilesIcon },
  ];

  return (
    <header className="h-12 bg-gray-900/80 backdrop-blur border-b border-gray-800 flex items-center justify-between px-4 shrink-0">
      {/* Left: Session picker + title */}
      <div className="flex items-center gap-3">
        <SessionPicker
          sessions={sessions}
          currentSessionId={currentSessionId}
          onSelect={onSelectSession}
          onNew={onNewSession}
          onDelete={onDeleteSession}
        />
        <div className="h-5 w-px bg-gray-800" />
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

      {/* Right: connection + activity toggle */}
      <div className="flex items-center gap-3">
        <div className="flex items-center gap-2">
          <span className={`inline-block w-2 h-2 rounded-full ${connected ? 'bg-green-500' : 'bg-red-500'}`} />
          <span className={`text-xs ${connected ? 'text-gray-500' : 'text-red-400'}`}>
            {connected ? 'Connected' : 'Disconnected'}
          </span>
        </div>
        <button
          onClick={onToggleActivity}
          className={`w-8 h-8 flex items-center justify-center rounded-lg transition-colors relative ${
            activityPanelOpen ? 'bg-gray-800 text-indigo-400' : 'text-gray-500 hover:bg-gray-800 hover:text-gray-300'
          }`}
          title="Toggle activity panel"
        >
          <ActivityIcon className="w-4 h-4" />
          {activityCount > 0 && (
            <span className="absolute -top-1 -right-1 w-4 h-4 bg-indigo-500 text-white rounded-full text-[10px] flex items-center justify-center font-bold">
              {activityCount > 99 ? '99' : activityCount}
            </span>
          )}
        </button>
      </div>
    </header>
  );
}

function AgentIcon({ className }) {
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

function ActivityIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 6.75h16.5M3.75 12h16.5M12 17.25h8.25" />
    </svg>
  );
}
