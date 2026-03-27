import { useEffect, useState } from 'react'
import { PipelineRow } from '../components/PipelineRow'
import { fetchPipelines, fetchEnvironments } from '../api'
import type { PipelineRun } from '../types'

export function Pipelines() {
  const [pipelines, setPipelines] = useState<PipelineRun[]>([])
  const [filterApp, setFilterApp] = useState<string>('')
  const [filterEnv, setFilterEnv] = useState<string>('')
  const [appSlugs, setAppSlugs] = useState<string[]>([])
  const [envSlugs, setEnvSlugs] = useState<string[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setLoading(true)
    setError(null)
    Promise.all([fetchPipelines(), fetchEnvironments()])
      .then(([pipes, envs]) => {
        setPipelines(pipes)
        setEnvSlugs(envs.map((e) => e.slug))
        const allAppSlugs = new Set<string>()
        for (const env of envs) {
          for (const app of env.apps || []) {
            allAppSlugs.add(app.slug)
          }
        }
        for (const p of pipes) {
          allAppSlugs.add(p.app_slug)
        }
        setAppSlugs([...allAppSlugs].sort())
      })
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false))
  }, [])

  const filtered = pipelines.filter((p) => {
    if (filterApp && p.app_slug !== filterApp) return false
    if (filterEnv && p.target_env !== filterEnv && p.source_env !== filterEnv) return false
    return true
  })

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-center">
          <div className="w-6 h-6 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin mx-auto mb-3" />
          <p className="text-white/30 text-sm">Loading pipelines...</p>
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-[#e2e8f0]">Pipelines</h1>
          <p className="text-sm text-white/40 mt-1">{pipelines.length} pipeline runs</p>
        </div>
        <button className="px-4 py-2 bg-[#7c3aed] hover:bg-[#6d28d9] text-white text-sm font-medium rounded-lg transition-colors">
          New Pipeline
        </button>
      </div>

      {error && (
        <div className="bg-amber-500/10 border border-amber-500/30 rounded-xl p-4">
          <p className="text-sm text-amber-400">API error (showing cached/mock data): {error}</p>
        </div>
      )}

      {/* Filters */}
      <div className="flex items-center gap-3">
        <select
          value={filterApp}
          onChange={(e) => setFilterApp(e.target.value)}
          className="rounded-lg px-3 py-1.5 text-sm text-white/60 border border-white/10 focus:outline-none focus:ring-2 focus:ring-[#7c3aed]/50 focus:border-transparent cursor-pointer"
          style={{ background: '#1e1e3a' }}
        >
          <option value="">All apps</option>
          {appSlugs.map((s) => (
            <option key={s} value={s}>{s}</option>
          ))}
        </select>
        <select
          value={filterEnv}
          onChange={(e) => setFilterEnv(e.target.value)}
          className="rounded-lg px-3 py-1.5 text-sm text-white/60 border border-white/10 focus:outline-none focus:ring-2 focus:ring-[#7c3aed]/50 focus:border-transparent cursor-pointer"
          style={{ background: '#1e1e3a' }}
        >
          <option value="">All environments</option>
          {envSlugs.map((s) => (
            <option key={s} value={s}>{s}</option>
          ))}
        </select>
        {(filterApp || filterEnv) && (
          <button
            onClick={() => { setFilterApp(''); setFilterEnv('') }}
            className="text-xs text-white/30 hover:text-white/50"
          >
            Clear filters
          </button>
        )}
      </div>

      {/* Pipeline List */}
      <div className="space-y-2">
        {filtered.length === 0 ? (
          <div className="rounded-xl border border-white/5 p-8 text-center" style={{ background: '#1e1e3a' }}>
            <p className="text-sm text-white/30">No pipeline runs match your filters.</p>
          </div>
        ) : (
          filtered.map((p) => <PipelineRow key={p.id} pipeline={p} />)
        )}
      </div>
    </div>
  )
}
