import { useState } from "react";
import { useLocation } from "react-router-dom";
import Sidebar from "./Sidebar";
import { Menu, Code2, ExternalLink, Play, Square, RefreshCw } from "lucide-react";
import TaskBell from "./tasks/TaskBell";
import TaskDropdown from "./tasks/TaskDropdown";
import Studio, { CODESERVER_BASE, statusDot } from "../pages/Studio";
import { useStudio } from "../context/StudioContext";

function StudioHeaderInfo() {
  const { currentApp, status, selectedSlug, activeTab, busy, onControl } = useStudio();
  if (!currentApp) return null;

  const state = (status?.state || currentApp.state || 'stopped').toLowerCase();
  const isRunning = state === 'running';
  const domain = currentApp.domain || `${currentApp.slug}.mynetwk.biz`;
  const uptime = status?.uptime_secs != null
    ? `${Math.floor(status.uptime_secs / 60)}m ${status.uptime_secs % 60}s`
    : '-';

  return (
    <div className="flex items-center gap-3 min-w-0">
      <div className="flex items-center gap-2 shrink-0">
        <Code2 className="w-4 h-4 text-blue-400" />
        <span className={`w-2 h-2 rounded-full ${statusDot(state)}`} />
        <span className="text-[13px] font-medium text-white truncate max-w-[140px]">{currentApp.name}</span>
        <span className="px-1.5 py-0.5 rounded text-[10px] bg-gray-700 text-gray-400">{currentApp.stack}</span>
      </div>
      <a
        href={`https://${domain}`}
        target="_blank"
        rel="noopener noreferrer"
        className="hidden md:flex items-center gap-1 text-[11px] text-blue-400 hover:text-blue-300 truncate max-w-[200px]"
        title={domain}
      >
        <span className="truncate">{domain}</span>
        <ExternalLink className="w-3 h-3 shrink-0" />
      </a>
      <div className="hidden lg:flex items-center gap-3 text-[11px] text-gray-400 shrink-0">
        <span>PID <span className="text-gray-200 font-mono">{status?.pid || '-'}</span></span>
        <span>Port <span className="text-gray-200 font-mono">{currentApp.port || '-'}</span></span>
        <span>Up <span className="text-gray-200 font-mono">{uptime}</span></span>
      </div>
      {activeTab === 'code' && selectedSlug && (
        <a
          href={`${CODESERVER_BASE}/?folder=/opt/homeroute/apps/${selectedSlug}/src`}
          target="_blank"
          rel="noopener noreferrer"
          className="p-1 text-gray-400 hover:text-white rounded hover:bg-gray-700 shrink-0"
          title="Ouvrir code-server dans un nouvel onglet"
        >
          <ExternalLink className="w-3.5 h-3.5" />
        </a>
      )}
      {onControl && (
        <div className="flex items-center gap-1 shrink-0">
          {!isRunning ? (
            <button
              onClick={() => onControl('start')}
              disabled={busy}
              className="p-1 text-green-400 hover:bg-gray-700 rounded disabled:opacity-50"
              title="Démarrer"
            >
              <Play className="w-3.5 h-3.5" />
            </button>
          ) : (
            <button
              onClick={() => onControl('stop')}
              disabled={busy}
              className="p-1 text-yellow-400 hover:bg-gray-700 rounded disabled:opacity-50"
              title="Arrêter"
            >
              <Square className="w-3.5 h-3.5" />
            </button>
          )}
          <button
            onClick={() => onControl('restart')}
            disabled={busy}
            className="p-1 text-blue-400 hover:bg-gray-700 rounded disabled:opacity-50"
            title="Redémarrer"
          >
            <RefreshCw className="w-3.5 h-3.5" />
          </button>
        </div>
      )}
    </div>
  );
}

function Layout({ children }) {
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const location = useLocation();
  const isStudio = location.pathname === '/studio';

  return (
    <div className="flex h-screen">
      {sidebarOpen && (
        <div
          className="fixed inset-0 bg-black/60 z-40 lg:hidden"
          onClick={() => setSidebarOpen(false)}
        />
      )}

      <div
        className={`fixed inset-y-0 left-0 z-50 w-64 transform transition-transform duration-200 ease-out lg:relative lg:translate-x-0 ${
          sidebarOpen ? "translate-x-0" : "-translate-x-full"
        }`}
      >
        <Sidebar onClose={() => setSidebarOpen(false)} />
      </div>

      <div className="flex-1 flex flex-col min-w-0">
        <div className="flex items-center justify-between gap-3 px-4 py-2 bg-gray-800 border-b border-gray-700">
          <div className="flex items-center gap-3 min-w-0">
            <button
              onClick={() => setSidebarOpen(true)}
              className="lg:hidden p-1 text-gray-400 hover:text-white shrink-0"
            >
              <Menu className="w-6 h-6" />
            </button>
            <h1 className="lg:hidden text-lg font-bold shrink-0">HomeRoute</h1>
            <StudioHeaderInfo />
          </div>
          <div className="relative shrink-0">
            <TaskBell />
            <TaskDropdown />
          </div>
        </div>
        <main className="flex-1 overflow-hidden relative">
          <div
            className={isStudio ? "absolute inset-0" : "hidden"}
            aria-hidden={!isStudio}
          >
            <Studio />
          </div>
          {!isStudio && (
            <div className="h-full overflow-auto">{children}</div>
          )}
        </main>
      </div>
    </div>
  );
}

export default Layout;
