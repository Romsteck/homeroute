import type { EnvApp } from "../types";

interface Props {
  apps: EnvApp[];
  selectedApp: string | null;
  onSelectApp: (slug: string) => void;
}

export function ProjectSidebar({ apps, selectedApp, onSelectApp }: Props) {
  return (
    <aside className="w-[200px] min-w-[200px] h-full bg-sidebar border-r border-border flex flex-col overflow-hidden">
      <div className="px-3 pt-4 pb-2 text-[10px] font-semibold uppercase tracking-wider text-muted">
        Projects
      </div>
      <div className="flex-1 overflow-y-auto px-2 pb-2">
        {apps.map((app) => {
          const sel = app.slug === selectedApp;
          return (
            <button
              key={app.slug}
              onClick={() => onSelectApp(app.slug)}
              className={`flex items-center gap-2.5 w-full px-2.5 py-2 mb-0.5 rounded-lg text-[13px] text-left transition-colors cursor-pointer border-none ${
                sel ? "bg-surface text-txt" : "bg-transparent text-txt2 hover:bg-surface/50"
              }`}
            >
              <span className="flex-1 truncate">{app.name}</span>
              <span
                className={`w-[7px] h-[7px] rounded-full shrink-0 ${
                  app.status === "running" ? "bg-ok" : "bg-err"
                }`}
              />
            </button>
          );
        })}
      </div>
    </aside>
  );
}
