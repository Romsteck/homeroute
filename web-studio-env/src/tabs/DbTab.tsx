import { useState, useEffect, useCallback } from "react";
import type { Environment, DbTable, DbQueryResult } from "../types";
import { getDbTables, queryDb } from "../api";

interface Props { env: Environment; appSlug: string; }

export function DbTab({ env, appSlug }: Props) {
  const [tables, setTables] = useState<DbTable[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<DbQueryResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [querying, setQuerying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [qError, setQError] = useState<string | null>(null);
  const isProd = env.type === "prod";

  const fetch_ = useCallback(async () => {
    try { setTables(await getDbTables(env.slug, appSlug)); setError(null); }
    catch (err) {
      setError(`Could not load tables: ${err instanceof Error ? err.message : "?"}`);
      setTables([
        { name: "users", row_count: 42, columns: [{ name: "id", type: "INTEGER", nullable: false, primary_key: true }, { name: "email", type: "TEXT", nullable: false, primary_key: false }] },
        { name: "sessions", row_count: 156, columns: [{ name: "id", type: "TEXT", nullable: false, primary_key: true }, { name: "user_id", type: "INTEGER", nullable: false, primary_key: false }] },
      ]);
    } finally { setLoading(false); }
  }, [env.slug, appSlug]);

  useEffect(() => { setLoading(true); setSelected(null); setResult(null); setQuery(""); fetch_(); }, [fetch_]);

  const selectTable = (name: string) => { setSelected(name); setQuery(`SELECT * FROM ${name} LIMIT 50`); setResult(null); setQError(null); };

  const runQuery = async () => {
    if (!query.trim()) return;
    if (isProd && !/^\s*SELECT\b/i.test(query)) { setQError("Only SELECT queries in production."); return; }
    setQuerying(true); setQError(null);
    try { setResult(await queryDb(env.slug, query, appSlug)); }
    catch (err) { setQError(err instanceof Error ? err.message : "Query failed"); setResult({ columns: ["id", "email"], rows: [{ id: 1, email: "demo@example.com" }], row_count: 1 }); }
    finally { setQuerying(false); }
  };

  if (loading) return <div className="flex items-center justify-center h-full text-muted text-sm">Loading database...</div>;

  return (
    <div className="flex h-full overflow-hidden">
      {/* Table list sidebar */}
      <div className="w-56 shrink-0 border-r border-border overflow-y-auto p-4">
        <h3 className="text-[10px] font-semibold uppercase tracking-wider mb-3 text-muted">Tables</h3>
        {error && <div className="text-xs mb-2 text-warn">{error}</div>}
        {tables.map(t => (
          <button key={t.name} onClick={() => selectTable(t.name)}
            className={`flex items-center justify-between w-full px-3 py-2 rounded text-sm text-left cursor-pointer border-none transition-colors ${
              selected === t.name ? "bg-surface text-txt" : "bg-transparent text-txt2 hover:bg-surface/50"
            }`}>
            <span className="truncate">{t.name}</span>
            <span className="text-xs text-muted">{t.row_count}</span>
          </button>
        ))}
        {tables.length === 0 && <div className="text-xs py-4 text-center text-muted">No tables</div>}
        {selected && (
          <div className="mt-4 pt-4 border-t border-border">
            <h4 className="text-[10px] font-semibold uppercase tracking-wider mb-2 text-muted">Columns</h4>
            {tables.find(t => t.name === selected)?.columns.map(col => (
              <div key={col.name} className="flex items-center gap-2 py-1">
                {col.primary_key && <span className="text-[10px] text-warn">PK</span>}
                <span className="text-xs text-txt2">{col.name}</span>
                <span className="text-[10px] ml-auto text-muted">{col.type}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Query area */}
      <div className="flex flex-col flex-1 min-w-0 p-4">
        <div className="flex gap-2 mb-4">
          <textarea value={query} onChange={e => setQuery(e.target.value)} placeholder="SELECT * FROM ..."
            className="flex-1 p-3 rounded text-sm font-mono resize-none outline-none bg-surface text-txt border border-border h-20"
            onKeyDown={e => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) runQuery(); }} />
          <button onClick={runQuery} disabled={querying || !query.trim()}
            className="px-4 rounded text-sm font-medium self-end h-10 bg-accent text-white cursor-pointer border-none disabled:opacity-50">
            {querying ? "..." : "Run"}
          </button>
        </div>
        {isProd && <div className="mb-3 px-3 py-1.5 rounded text-xs bg-err/10 text-err">Production: SELECT queries only</div>}
        {qError && <div className="mb-3 px-3 py-2 rounded text-xs bg-err/10 text-err">{qError}</div>}
        {result && (
          <div className="flex-1 overflow-auto rounded border border-border">
            <table className="w-full text-sm">
              <thead>
                <tr className="bg-surface">
                  {result.columns.map(col => (
                    <th key={col} className="px-3 py-2 text-left text-xs font-semibold text-muted border-b border-border">{col}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {result.rows.map((row, i) => (
                  <tr key={i} className={i % 2 ? "bg-surface/30" : ""}>
                    {result.columns.map(col => (
                      <td key={col} className="px-3 py-2 text-xs font-mono text-txt2 border-b border-border">{String(row[col] ?? "")}</td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="px-3 py-2 text-xs bg-surface text-muted">{result.row_count} row{result.row_count !== 1 ? "s" : ""}</div>
          </div>
        )}
        {!result && !qError && <div className="flex-1 flex items-center justify-center text-muted text-sm">Select a table or run a query (Ctrl+Enter)</div>}
      </div>
    </div>
  );
}
