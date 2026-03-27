import type { TabId } from "../types";

const TABS: { id: TabId; label: string }[] = [
  { id: "code", label: "Code" },
  { id: "board", label: "Board" },
  { id: "docs", label: "Docs" },
  { id: "pipes", label: "Pipes" },
  { id: "db", label: "DB" },
  { id: "logs", label: "Logs" },
];

interface Props {
  activeTab: TabId;
  onSelectTab: (tab: TabId) => void;
  isProd: boolean;
}

export function TabBar({ activeTab, onSelectTab, isProd }: Props) {
  return (
    <div className="flex items-center h-[38px] shrink-0 bg-sidebar border-b border-border pl-4">
      {TABS.map((tab) => {
        const active = tab.id === activeTab;
        return (
          <button
            key={tab.id}
            onClick={() => onSelectTab(tab.id)}
            className={`relative h-full px-4 border-none cursor-pointer text-[13px] bg-transparent transition-colors ${
              active ? "text-txt font-medium" : "text-muted hover:text-txt2"
            }`}
          >
            {tab.label}
            {isProd && tab.id === "code" && " 🔒"}
            {active && (
              <span className="absolute bottom-0 left-3 right-3 h-0.5 rounded-full bg-accent" />
            )}
          </button>
        );
      })}
    </div>
  );
}
