import { useState, useEffect, useRef, useCallback } from 'react';

export default function DocsPanel({ sendRaw, subscribe, connected }) {
  const [docs, setDocs] = useState(null);
  const [selected, setSelected] = useState({ type: 'overview' });
  const versionRef = useRef(null);
  const refreshTimerRef = useRef(null);

  const requestDocs = useCallback(() => {
    if (sendRaw) {
      sendRaw({ type: 'read_file', path: 'docs.json' });
    }
  }, [sendRaw]);

  // Subscribe to file_content for docs.json
  useEffect(() => {
    if (!subscribe || !sendRaw) return;

    const unsub = subscribe('file_content', (data) => {
      if (data.path !== 'docs.json') return;
      if (!data.content) {
        setDocs(null);
        return;
      }
      try {
        const parsed = JSON.parse(data.content);
        if (parsed.version !== versionRef.current) {
          versionRef.current = parsed.version;
          setDocs(parsed);
        }
      } catch {
        setDocs(null);
      }
    });

    requestDocs();

    return unsub;
  }, [subscribe, sendRaw, requestDocs]);

  // Re-fetch when connection is (re)established (with delay to ensure WS is ready)
  useEffect(() => {
    if (!connected) return;
    const t = setTimeout(requestDocs, 300);
    return () => clearTimeout(t);
  }, [connected, requestDocs]);

  // Auto-refresh every 30s
  useEffect(() => {
    refreshTimerRef.current = setInterval(requestDocs, 30000);
    return () => clearInterval(refreshTimerRef.current);
  }, [requestDocs]);

  // Refresh on visibility change
  useEffect(() => {
    const handler = () => {
      if (!document.hidden) requestDocs();
    };
    document.addEventListener('visibilitychange', handler);
    return () => document.removeEventListener('visibilitychange', handler);
  }, [requestDocs]);

  const navigateToFlow = useCallback((flowId) => {
    setSelected({ type: 'flow', id: flowId });
  }, []);

  if (!connected) {
    return (
      <div className="flex-1 flex items-center justify-center bg-gray-900 text-gray-500">
        <span className="text-sm">Disconnected</span>
      </div>
    );
  }

  if (!docs) {
    return <EmptyState />;
  }

  return (
    <div className="flex-1 flex min-h-0 bg-gray-900">
      <DocsSidebar
        docs={docs}
        selected={selected}
        onSelect={setSelected}
      />
      <div className="flex-1 overflow-y-auto p-6">
        {selected.type === 'overview' && (
          <AppOverviewCard app={docs.app} screens={docs.screens} flows={docs.flows} />
        )}
        {selected.type === 'screen' && (
          <ScreenDetail
            screen={(docs.screens || []).find(s => s.id === selected.id)}
            onNavigateFlow={navigateToFlow}
          />
        )}
        {selected.type === 'flow' && (
          <FlowDiagram flow={(docs.flows || []).find(f => f.id === selected.id)} />
        )}
      </div>
    </div>
  );
}

function EmptyState() {
  return (
    <div className="flex-1 flex flex-col items-center justify-center bg-gray-900 text-gray-500 gap-4">
      <svg className="w-16 h-16 text-gray-700" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1}>
        <path strokeLinecap="round" strokeLinejoin="round" d="M12 6.042A8.967 8.967 0 006 3.75c-1.052 0-2.062.18-3 .512v14.25A8.987 8.987 0 016 18c2.305 0 4.408.867 6 2.292m0-14.25a8.966 8.966 0 016-2.292c1.052 0 2.062.18 3 .512v14.25A8.987 8.987 0 0018 18a8.967 8.967 0 00-6 2.292m0-14.25v14.25" />
      </svg>
      <div className="text-center max-w-sm">
        <p className="text-gray-400 text-sm mb-2">Aucune documentation pour le moment.</p>
        <p className="text-gray-600 text-xs leading-relaxed">
          L'agent créera automatiquement la documentation au fur et à mesure
          du développement. Vous pouvez aussi demander à l'agent de documenter
          l'état actuel de l'application.
        </p>
      </div>
    </div>
  );
}

function DocsSidebar({ docs, selected, onSelect }) {
  const screens = docs.screens || [];
  const flows = docs.flows || [];

  return (
    <div className="w-60 shrink-0 border-r border-gray-800 overflow-y-auto bg-gray-900">
      {/* Overview */}
      <div className="p-2">
        <button
          onClick={() => onSelect({ type: 'overview' })}
          className={`w-full text-left px-3 py-2 rounded-lg text-sm font-medium transition-colors ${
            selected.type === 'overview'
              ? 'bg-indigo-600/15 text-indigo-400'
              : 'text-gray-300 hover:bg-gray-800/50'
          }`}
        >
          Overview
        </button>
      </div>

      {/* Screens */}
      {screens.length > 0 && (
        <div className="px-2 pb-2">
          <div className="px-3 py-1.5 text-xs font-semibold text-gray-500 uppercase tracking-wider">
            Screens
          </div>
          {screens.map(screen => (
            <button
              key={screen.id}
              onClick={() => onSelect({ type: 'screen', id: screen.id })}
              className={`w-full text-left px-3 py-1.5 rounded-lg text-sm transition-colors ${
                selected.type === 'screen' && selected.id === screen.id
                  ? 'bg-indigo-600/15 text-indigo-400'
                  : 'text-gray-400 hover:bg-gray-800/50 hover:text-gray-300'
              }`}
            >
              {screen.name}
            </button>
          ))}
        </div>
      )}

      {/* Flows */}
      {flows.length > 0 && (
        <div className="px-2 pb-2">
          <div className="px-3 py-1.5 text-xs font-semibold text-gray-500 uppercase tracking-wider">
            Flows
          </div>
          {flows.map(flow => (
            <button
              key={flow.id}
              onClick={() => onSelect({ type: 'flow', id: flow.id })}
              className={`w-full text-left px-3 py-1.5 rounded-lg text-sm transition-colors ${
                selected.type === 'flow' && selected.id === flow.id
                  ? 'bg-indigo-600/15 text-indigo-400'
                  : 'text-gray-400 hover:bg-gray-800/50 hover:text-gray-300'
              }`}
            >
              {flow.name}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function AppOverviewCard({ app, screens, flows }) {
  if (!app) return null;

  return (
    <div className="max-w-2xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold text-gray-100">{app.name}</h1>
        {app.description && (
          <p className="text-gray-400 mt-1">{app.description}</p>
        )}
      </div>

      {app.business_context && (
        <div>
          <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">Business Context</h2>
          <p className="text-sm text-gray-300 leading-relaxed">{app.business_context}</p>
        </div>
      )}

      {app.target_users && app.target_users.length > 0 && (
        <div>
          <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">Target Users</h2>
          <div className="flex flex-wrap gap-2">
            {app.target_users.map((user, i) => (
              <span key={i} className="px-2.5 py-1 rounded-full text-xs font-medium bg-indigo-500/20 text-indigo-300">
                {user}
              </span>
            ))}
          </div>
        </div>
      )}

      <div className="flex gap-6 pt-2">
        <div className="text-center">
          <div className="text-2xl font-semibold text-gray-200">{(screens || []).length}</div>
          <div className="text-xs text-gray-500">Screens</div>
        </div>
        <div className="text-center">
          <div className="text-2xl font-semibold text-gray-200">{(flows || []).length}</div>
          <div className="text-xs text-gray-500">Flows</div>
        </div>
      </div>
    </div>
  );
}

function ScreenDetail({ screen, onNavigateFlow }) {
  if (!screen) return <p className="text-gray-500 text-sm">Screen not found.</p>;

  return (
    <div className="max-w-2xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold text-gray-100">{screen.name}</h1>
        {screen.path && (
          <span className="inline-block mt-1 px-2 py-0.5 rounded text-xs font-mono bg-gray-800 text-gray-300">
            {screen.path}
          </span>
        )}
      </div>

      {screen.description && (
        <p className="text-sm text-gray-300 leading-relaxed">{screen.description}</p>
      )}

      {screen.features && screen.features.length > 0 && (
        <div>
          <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">Features</h2>
          <ul className="space-y-1">
            {screen.features.map((feat, i) => (
              <li key={i} className="text-sm text-gray-300 flex items-start gap-2">
                <span className="text-indigo-400 mt-1 shrink-0">&#x2022;</span>
                {feat}
              </li>
            ))}
          </ul>
        </div>
      )}

      {screen.related_tables && screen.related_tables.length > 0 && (
        <div>
          <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">Related Tables</h2>
          <div className="flex flex-wrap gap-2">
            {screen.related_tables.map((table, i) => (
              <span key={i} className="px-2.5 py-1 rounded-full text-xs font-medium bg-emerald-500/20 text-emerald-300">
                {table}
              </span>
            ))}
          </div>
        </div>
      )}

      {screen.related_flows && screen.related_flows.length > 0 && (
        <div>
          <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">Related Flows</h2>
          <div className="flex flex-wrap gap-2">
            {screen.related_flows.map((flowId, i) => (
              <button
                key={i}
                onClick={() => onNavigateFlow(flowId)}
                className="px-2.5 py-1 rounded-full text-xs font-medium bg-blue-500/20 text-blue-300 hover:bg-blue-500/30 transition-colors cursor-pointer"
              >
                {flowId}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function FlowDiagram({ flow }) {
  if (!flow) return <p className="text-gray-500 text-sm">Flow not found.</p>;

  const steps = flow.steps || [];

  return (
    <div className="max-w-2xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold text-gray-100">{flow.name}</h1>
        {flow.description && (
          <p className="text-gray-400 mt-1">{flow.description}</p>
        )}
      </div>

      <div className="flex flex-col items-center gap-0">
        {steps.map((step, i) => (
          <div key={step.id} className="flex flex-col items-center">
            {i > 0 && <Connector />}
            {step.type === 'decision' ? (
              <DecisionNode step={step} steps={steps} />
            ) : (
              <StepNode step={step} />
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function StepNode({ step }) {
  const styles = step.type === 'state'
    ? 'border-blue-500 bg-blue-500/10'
    : 'border-green-500 bg-green-500/10';

  return (
    <div className={`px-6 py-3 border rounded-lg text-sm text-gray-200 min-w-[180px] text-center ${styles}`}>
      {step.label}
    </div>
  );
}

function DecisionNode({ step, steps }) {
  const outcomes = step.outcomes || [];

  return (
    <div className="flex flex-col items-center">
      {/* Diamond shape */}
      <div className="relative w-[120px] h-[120px] flex items-center justify-center">
        <div className="absolute inset-0 border-2 border-amber-500 bg-amber-500/10 transform rotate-45 rounded-sm" />
        <span className="relative text-sm text-gray-200 text-center px-2 z-10">{step.label}</span>
      </div>

      {/* Outcomes */}
      {outcomes.length > 0 && (
        <>
          <Connector />
          <div className="flex gap-8 items-start">
            {outcomes.map((outcome, i) => {
              const targetStep = steps.find(s => s.id === outcome.next);
              return (
                <div key={i} className="flex flex-col items-center gap-1">
                  <span className="text-xs text-amber-400 font-medium">{outcome.label}</span>
                  <div className="w-px h-4 bg-gray-600" />
                  {targetStep ? (
                    <div className="px-3 py-1.5 border border-gray-700 rounded text-xs text-gray-400 bg-gray-800/50">
                      {targetStep.label}
                    </div>
                  ) : (
                    <span className="text-xs text-gray-600">{outcome.next}</span>
                  )}
                </div>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}

function Connector() {
  return <div className="w-px h-8 bg-gray-600" />;
}
