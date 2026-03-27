import { useState, useEffect, useCallback } from "react";
import type { Todo } from "../types";
import { getTodos, completeTodo, updateTodoStatus } from "../api";

interface Props { appSlug: string; }

const COLUMNS = [
  { key: "todo", label: "Todo", statuses: ["todo"] },
  { key: "in_progress", label: "In Progress", statuses: ["in_progress"] },
  { key: "done", label: "Done", statuses: ["done"] },
];

export function BoardTab({ appSlug }: Props) {
  const [todos, setTodos] = useState<Todo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchAll = useCallback(async () => {
    try {
      setTodos(await getTodos(appSlug));
      setError(null);
    } catch (err) {
      setError(`Could not load todos: ${err instanceof Error ? err.message : "?"}`);
      setTodos([
        { id: "1", title: "Fix login redirect loop", description: "Users get stuck", context: appSlug, priority: "high", status: "todo", created_at: new Date().toISOString() },
        { id: "2", title: "Add dark mode toggle", context: appSlug, priority: "medium", status: "in_progress", created_at: new Date().toISOString() },
        { id: "3", title: "Update dependencies", context: appSlug, priority: "low", status: "done", created_at: new Date().toISOString() },
      ]);
    } finally { setLoading(false); }
  }, [appSlug]);

  useEffect(() => { setLoading(true); fetchAll(); }, [fetchAll]);

  const handleComplete = async (id: string) => {
    try { await completeTodo(id); } catch { /* ok */ }
    setTodos(p => p.map(t => t.id === id ? { ...t, status: "done" as const } : t));
  };
  const handleStart = async (id: string) => {
    try { await updateTodoStatus(id, "in_progress"); } catch { /* ok */ }
    setTodos(p => p.map(t => t.id === id ? { ...t, status: "in_progress" as const } : t));
  };

  if (loading) return <div className="flex items-center justify-center h-full text-muted text-sm">Loading board...</div>;

  return (
    <div className="flex flex-col h-full p-6 overflow-auto">
      {error && <div className="mb-4 px-3 py-2 rounded text-xs bg-warn/10 text-warn">{error}</div>}
      <div className="flex gap-4 flex-1 min-h-0">
        {COLUMNS.map(col => {
          const items = todos.filter(t => col.statuses.includes(t.status));
          return (
            <div key={col.key} className="flex flex-col flex-1 min-w-[220px]">
              <div className="flex items-center gap-2 mb-3">
                <h3 className="text-sm font-semibold text-txt">{col.label}</h3>
                <span className="px-1.5 py-0.5 rounded text-xs bg-surface text-muted">{items.length}</span>
              </div>
              <div className="flex flex-col gap-2 flex-1 p-2 rounded-lg overflow-y-auto bg-bg/50">
                {items.map(todo => (
                  <div key={todo.id} className="p-3 rounded-lg bg-surface border border-border">
                    <div className="flex items-start justify-between gap-2 mb-1">
                      <span className="text-sm font-medium text-txt">{todo.title}</span>
                      <span className={`px-1.5 py-0.5 rounded text-[10px] uppercase font-semibold shrink-0 ${
                        todo.priority === "high" ? "bg-err/15 text-err" :
                        todo.priority === "medium" ? "bg-warn/15 text-warn" : "bg-muted/15 text-muted"
                      }`}>{todo.priority}</span>
                    </div>
                    {todo.description && <p className="text-xs mb-2 line-clamp-2 text-muted">{todo.description}</p>}
                    <div className="flex gap-1 mt-2">
                      {todo.status === "todo" && (
                        <button onClick={() => handleStart(todo.id)} className="px-2 py-0.5 rounded text-[10px] bg-accent/15 text-accent-light cursor-pointer border-none">Start</button>
                      )}
                      {todo.status !== "done" && (
                        <button onClick={() => handleComplete(todo.id)} className="px-2 py-0.5 rounded text-[10px] bg-ok/15 text-ok cursor-pointer border-none">Complete</button>
                      )}
                    </div>
                  </div>
                ))}
                {items.length === 0 && <div className="text-xs text-center py-6 text-muted">No items</div>}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
