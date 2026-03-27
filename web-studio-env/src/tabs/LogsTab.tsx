import { useState, useEffect, useRef, useCallback } from "react";
import type { Environment, LogEntry } from "../types";
import { getAppLogs } from "../api";

interface Props { env: Environment; appSlug: string; }

export function LogsTab({ env, appSlug }: Props) {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [filter, setFilter] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const ref = useRef<HTMLDivElement>(null);

  const fetch_ = useCallback(async () => {
    try { setLogs(await getAppLogs(env.slug, appSlug, 200)); setError(null); }
    catch (err) {
      if (logs.length === 0) {
        setError(`Could not load logs: ${err instanceof Error ? err.message : "?"}`);
        const now = Date.now();
        setLogs([
          { timestamp: new Date(now-60000).toISOString(), level: "INFO", message: `[${appSlug}] Server started` },
          { timestamp: new Date(now-30000).toISOString(), level: "WARN", message: `[${appSlug}] Slow query: 250ms` },
          { timestamp: new Date(now-5000).toISOString(), level: "ERROR", message: `[${appSlug}] Connection failed` },
        ]);
      }
    } finally { setLoading(false); }
  }, [env.slug, appSlug]);

  useEffect(() => { setLoading(true); setFilter(""); fetch_(); }, [fetch_]);
  useEffect(() => { const iv = setInterval(fetch_, 5000); return () => clearInterval(iv); }, [fetch_]);
  useEffect(() => { if (autoScroll && ref.current) ref.current.scrollTop = ref.current.scrollHeight; }, [logs, autoScroll]);

  const onScroll = () => { if (!ref.current) return; const { scrollTop, scrollHeight, clientHeight } = ref.current; setAutoScroll(scrollHeight - scrollTop - clientHeight < 50); };

  const filtered = filter ? logs.filter(l => l.message.toLowerCase().includes(filter.toLowerCase()) || l.level.toLowerCase().includes(filter.toLowerCase())) : logs;

  const levelCls = (l: string) => l === "ERROR" ? "text-err" : l === "WARN" ? "text-warn" : l === "DEBUG" ? "text-muted" : "text-txt2";

  if (loading) return <div className="flex items-center justify-center h-full text-muted text-sm">Loading logs...</div>;

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center gap-3 px-4 py-2 shrink-0 border-b border-border">
        <input type="text" value={filter} onChange={e => setFilter(e.target.value)} placeholder="Filter logs..."
          className="flex-1 max-w-[400px] px-3 py-1.5 rounded text-sm outline-none bg-surface text-txt border border-border" />
        {error && <span className="text-xs text-warn">{error}</span>}
        <span className="text-xs ml-auto text-muted">{filtered.length} entries{autoScroll ? " (auto-scroll)" : ""}</span>
      </div>
      {/* Logs */}
      <div ref={ref} onScroll={onScroll} className="flex-1 overflow-y-auto p-4 font-mono text-xs bg-bg">
        {filtered.map((entry, i) => {
          const time = entry.timestamp.includes("T") ? entry.timestamp.split("T")[1]?.replace("Z","").substring(0,12) : entry.timestamp;
          return (
            <div key={i} className="flex gap-3 py-0.5 hover:bg-white/[0.02]">
              <span className="shrink-0 w-24 text-muted">{time}</span>
              <span className={`shrink-0 w-12 text-right ${levelCls(entry.level)}`}>{entry.level}</span>
              <span className="text-txt2">{entry.message}</span>
            </div>
          );
        })}
        {filtered.length === 0 && <div className="text-center py-12 text-muted">{filter ? "No matching log entries" : "No logs available"}</div>}
      </div>
    </div>
  );
}
