import { useState, useEffect, useCallback } from "react";
import type { Environment, PipelineRun } from "../types";
import { getPipelines, triggerPromotion } from "../api";

interface Props { env: Environment; appSlug: string; }

function fmtDuration(ms: number) { return ms < 1000 ? `${ms}ms` : ms < 60000 ? `${Math.floor(ms/1000)}s` : `${Math.floor(ms/60000)}m`; }
function fmtTime(iso: string) { try { return new Date(iso).toLocaleString(); } catch { return iso; } }

const STS: Record<string, string> = { pending: "text-muted", running: "text-accent-light", success: "text-ok", failed: "text-err", skipped: "text-muted" };
const STBG: Record<string, string> = { pending: "bg-muted/15", running: "bg-accent/15", success: "bg-ok/15", failed: "bg-err/15", skipped: "bg-muted/10" };

export function PipesTab({ env, appSlug }: Props) {
  const [pipes, setPipes] = useState<PipelineRun[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [promoting, setPromoting] = useState(false);
  const canPromote = env.type === "dev" || env.type === "acc";

  const fetch_ = useCallback(async () => {
    try { setPipes(await getPipelines(appSlug)); setError(null); }
    catch (err) {
      setError(`Could not load pipelines: ${err instanceof Error ? err.message : "?"}`);
      setPipes([
        { id: "p1", app_slug: appSlug, status: "success", trigger: "git push", started_at: new Date(Date.now()-300000).toISOString(), finished_at: new Date(Date.now()-240000).toISOString(), steps: [{ name: "Build", status: "success", duration_ms: 32000 }, { name: "Test", status: "success", duration_ms: 12000 }, { name: "Deploy", status: "success", duration_ms: 8000 }] },
        { id: "p2", app_slug: appSlug, status: "failed", trigger: "manual", started_at: new Date(Date.now()-3600000).toISOString(), finished_at: new Date(Date.now()-3540000).toISOString(), steps: [{ name: "Build", status: "success", duration_ms: 29000 }, { name: "Test", status: "failed", duration_ms: 4500 }, { name: "Deploy", status: "skipped" }] },
      ]);
    } finally { setLoading(false); }
  }, [appSlug]);

  useEffect(() => { setLoading(true); fetch_(); }, [fetch_]);

  const handlePromote = async () => { setPromoting(true); try { await triggerPromotion(appSlug); } catch {} setPromoting(false); };

  if (loading) return <div className="flex items-center justify-center h-full text-muted text-sm">Loading pipelines...</div>;

  return (
    <div className="flex flex-col h-full p-6 overflow-y-auto">
      {error && <div className="mb-4 px-3 py-2 rounded text-xs bg-warn/10 text-warn">{error}</div>}
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-sm font-semibold text-txt">Pipeline Runs</h2>
        {canPromote && (
          <button onClick={handlePromote} disabled={promoting} className="px-4 py-2 rounded text-xs font-medium bg-accent text-white cursor-pointer border-none disabled:opacity-50">
            {promoting ? "Promoting..." : "Promote"}
          </button>
        )}
      </div>
      <div className="flex flex-col gap-4 max-w-4xl">
        {pipes.map(p => (
          <div key={p.id} className="rounded-lg overflow-hidden bg-surface border border-border">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border">
              <div className="flex items-center gap-3">
                <span className={`px-2 py-0.5 rounded text-xs font-medium uppercase ${STBG[p.status]} ${STS[p.status]}`}>{p.status}</span>
                <span className="text-sm text-txt">{p.trigger}</span>
              </div>
              <span className="text-xs text-muted">{fmtTime(p.started_at)}</span>
            </div>
            <div className="flex items-center gap-1 px-4 py-3">
              {p.steps.map((step, i) => (
                <div key={i} className="flex items-center gap-1">
                  {i > 0 && <div className="w-6 h-px bg-border" />}
                  <div className="flex items-center gap-2">
                    <span className={`w-2.5 h-2.5 rounded-full ${step.status === "success" ? "bg-ok" : step.status === "failed" ? "bg-err" : step.status === "running" ? "bg-accent" : "bg-muted"}`} />
                    <span className="text-xs text-txt2">{step.name}</span>
                    {step.duration_ms !== undefined && <span className="text-[10px] text-muted">{fmtDuration(step.duration_ms)}</span>}
                  </div>
                </div>
              ))}
            </div>
          </div>
        ))}
        {pipes.length === 0 && <div className="text-center py-12 text-muted">No pipeline runs yet</div>}
      </div>
    </div>
  );
}
