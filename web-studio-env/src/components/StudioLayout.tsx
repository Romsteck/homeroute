import { useState, useCallback } from "react";
import type { Environment, EnvApp, TabId } from "../types";
import { ProjectSidebar } from "./ProjectSidebar";
import { TabBar } from "./TabBar";
import { CodeTab } from "../tabs/CodeTab";
import { BoardTab } from "../tabs/BoardTab";
import { DocsTab } from "../tabs/DocsTab";
import { PipesTab } from "../tabs/PipesTab";
import { DbTab } from "../tabs/DbTab";
import { LogsTab } from "../tabs/LogsTab";

interface Props {
  env: Environment;
  currentApp: EnvApp | null;
  apps: EnvApp[];
  selectedApp: string | null;
  activeTab: TabId;
  onSelectApp: (slug: string) => void;
  onSelectTab: (tab: TabId) => void;
  error: string | null;
}

function renderTab(tab: TabId, env: Environment, _app: EnvApp | null, appSlug: string | null) {
  if (!appSlug) {
    return (
      <div className="flex items-center justify-center h-full text-muted text-sm">
        Select a project to get started
      </div>
    );
  }
  switch (tab) {
    case "board": return <BoardTab appSlug={appSlug} />;
    case "docs": return <DocsTab env={env} appSlug={appSlug} />;
    case "pipes": return <PipesTab env={env} appSlug={appSlug} />;
    case "db": return <DbTab env={env} appSlug={appSlug} />;
    case "logs": return <LogsTab env={env} appSlug={appSlug} />;
    default: return null;
  }
}

export function StudioLayout({ env, currentApp, apps, selectedApp, activeTab, onSelectApp, onSelectTab, error }: Props) {
  const isProd = env.type === "prod";
  // Track which app slugs have had their code-server opened (lazy-load)
  const [openedCodeServers, setOpenedCodeServers] = useState<Set<string>>(() => {
    // If we restore on "code" tab with a selected app, open it immediately
    const initial = new Set<string>();
    if (activeTab === "code" && selectedApp) initial.add(selectedApp);
    return initial;
  });

  const handleSelectTab = useCallback((tab: TabId) => {
    onSelectTab(tab);
    if (tab === "code" && selectedApp) {
      setOpenedCodeServers(prev => {
        if (prev.has(selectedApp)) return prev;
        const next = new Set(prev);
        next.add(selectedApp);
        return next;
      });
    }
  }, [onSelectTab, selectedApp]);

  const handleSelectApp = useCallback((slug: string) => {
    onSelectApp(slug);
    if (activeTab === "code") {
      setOpenedCodeServers(prev => {
        if (prev.has(slug)) return prev;
        const next = new Set(prev);
        next.add(slug);
        return next;
      });
    }
  }, [onSelectApp, activeTab]);

  return (
    <div className="flex w-screen h-screen overflow-hidden bg-bg">
      {/* Sidebar */}
      <ProjectSidebar apps={apps} selectedApp={selectedApp} onSelectApp={handleSelectApp} />

      {/* Main */}
      <div className="flex flex-col flex-1 min-w-0 h-full">
        {/* Header */}
        <header className="flex items-center justify-between h-[46px] shrink-0 px-5 bg-sidebar border-b border-border">
          <div className="flex items-center gap-2.5">
            <span className="w-2 h-2 rounded-full bg-ok" />
            <span className="text-[15px] font-semibold text-txt">Studio</span>
            <span className={`px-2.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide ${
              isProd ? "bg-err/15 text-err" : "bg-accent/20 text-accent-light"
            }`}>
              {env.name}
            </span>
          </div>
          {currentApp && (
            <div className="flex items-center gap-2">
              <span className="text-[13px] font-medium text-txt">{currentApp.name}</span>
              <span className="px-2 py-0.5 rounded text-[10px] bg-surface text-muted">
                {currentApp.stack}
              </span>
            </div>
          )}
        </header>

        {/* Error */}
        {error && (
          <div className="px-5 py-1.5 text-[11px] shrink-0 bg-warn/10 text-warn">
            {error}
          </div>
        )}

        {/* Tabs */}
        <TabBar activeTab={activeTab} onSelectTab={handleSelectTab} isProd={isProd} />

        {/* Content */}
        <div className="flex-1 overflow-hidden relative">
          {/* Code-server iframes: lazy-loaded on first click, then kept alive invisible */}
          {[...openedCodeServers].map(slug => {
            const isVisible = activeTab === "code" && selectedApp === slug;
            const app = apps.find(a => a.slug === slug) || null;
            return (
              <div
                key={slug}
                className="absolute inset-0"
                style={isVisible
                  ? { visibility: "visible", zIndex: 1 }
                  : { visibility: "hidden", zIndex: 0, pointerEvents: "none" }
                }
              >
                <CodeTab env={env} app={app} appSlug={slug} />
              </div>
            );
          })}
          {/* Other tabs: mounted/unmounted normally */}
          {activeTab !== "code" && (
            <div className="h-full">
              {renderTab(activeTab, env, currentApp, selectedApp)}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
