import { useState, useEffect, useCallback } from "react";
import type { Environment, AppDocs, DocSection } from "../types";
import { getAppDocs, updateAppDoc } from "../api";

interface Props { env: Environment; appSlug: string; }

const SECTIONS: { key: DocSection["section"]; label: string }[] = [
  { key: "meta", label: "Metadata" },
  { key: "structure", label: "Structure" },
  { key: "features", label: "Features" },
  { key: "backend", label: "Backend" },
  { key: "notes", label: "Notes" },
];

export function DocsTab({ env, appSlug }: Props) {
  const [docs, setDocs] = useState<AppDocs | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [editContent, setEditContent] = useState("");
  const [saving, setSaving] = useState(false);
  const isProd = env.type === "prod";

  const fetchDocs = useCallback(async () => {
    try { setDocs(await getAppDocs(appSlug)); setError(null); }
    catch (err) {
      setError(`Could not load docs: ${err instanceof Error ? err.message : "?"}`);
      setDocs({ app_id: appSlug, sections: SECTIONS.map(s => ({ section: s.key, content: "No documentation yet." })) });
    } finally { setLoading(false); }
  }, [appSlug]);

  useEffect(() => { setLoading(true); setEditing(null); fetchDocs(); }, [fetchDocs]);

  const handleSave = async () => {
    if (!editing) return;
    setSaving(true);
    try { await updateAppDoc(appSlug, editing, editContent); } catch { /* ok */ }
    setDocs(prev => prev ? { ...prev, sections: prev.sections.map(s => s.section === editing ? { ...s, content: editContent } : s) } : prev);
    setEditing(null);
    setSaving(false);
  };

  if (loading) return <div className="flex items-center justify-center h-full text-muted text-sm">Loading docs...</div>;

  const sectionsMap = new Map(docs?.sections.map(s => [s.section, s.content]) || []);

  return (
    <div className="flex flex-col h-full p-6 overflow-y-auto">
      {error && <div className="mb-4 px-3 py-2 rounded text-xs bg-warn/10 text-warn">{error}</div>}
      <div className="flex flex-col gap-6 max-w-4xl">
        {SECTIONS.map(({ key, label }) => {
          const content = sectionsMap.get(key) || "";
          const isEditing = editing === key;
          return (
            <div key={key} className="rounded-lg overflow-hidden bg-surface border border-border">
              <div className="flex items-center justify-between px-4 py-3 border-b border-border">
                <h3 className="text-sm font-semibold text-txt">{label}</h3>
                {!isProd && !isEditing && (
                  <button onClick={() => { setEditing(key); setEditContent(content); }} className="px-2 py-1 rounded text-xs text-accent-light cursor-pointer border-none bg-transparent hover:bg-accent/10">Edit</button>
                )}
              </div>
              <div className="p-4">
                {isEditing ? (
                  <div className="flex flex-col gap-2">
                    <textarea value={editContent} onChange={e => setEditContent(e.target.value)} className="w-full h-48 p-3 rounded text-sm font-mono resize-y outline-none bg-bg text-txt border border-border" />
                    <div className="flex gap-2 justify-end">
                      <button onClick={() => setEditing(null)} className="px-3 py-1.5 rounded text-xs text-muted cursor-pointer border-none bg-transparent">Cancel</button>
                      <button onClick={handleSave} disabled={saving} className="px-3 py-1.5 rounded text-xs text-white cursor-pointer border-none bg-accent">{saving ? "Saving..." : "Save"}</button>
                    </div>
                  </div>
                ) : (
                  <pre className={`text-sm whitespace-pre-wrap ${content ? "text-txt2" : "text-muted"} ${key === "meta" ? "font-mono" : ""}`}>
                    {content || "No content yet."}
                  </pre>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
