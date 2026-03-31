import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { AppTable } from '../components/AppRow'
import { PipelineRow } from '../components/PipelineRow'
import { fetchPipelines, fetchEnvironments, controlApp } from '../api'
import type { EnvApp, Environment, PipelineRun } from '../types'
import { isDevEnv } from '../types'

interface DashboardProps {
  currentEnv: string
}

export function Dashboard({ currentEnv }: DashboardProps) {
  const [apps, setApps] = useState<EnvApp[]>([])
  const [pipelines, setPipelines] = useState<PipelineRun[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [controlling, setControlling] = useState<string | null>(null)
  const [currentEnvData, setCurrentEnvData] = useState<Environment | null>(null)

  useEffect(() => {
    setLoading(true)
    setError(null)
    Promise.all([
      fetchEnvironments(),
      fetchPipelines(5),
    ])
      .then(([envs, p]) => {
        setPipelines(p)
        const env = envs.find((e) => e.slug === currentEnv)
        setCurrentEnvData(env ?? null)
        const isDev = env ? isDevEnv(env.env_type) : false
        const enrichedApps = (env?.apps || []).map((a) => ({
          ...a,
          status: a.running ? 'running' as const : 'stopped' as const,
          url: env?.slug ? `https://${a.slug}.${env.slug}.mynetwk.biz` : undefined,
          studio_url: isDev && env?.slug ? `https://studio.${env.slug}.mynetwk.biz/?folder=/apps/${a.slug}` : undefined,
        }))
        setApps(enrichedApps)
      })
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false))
  }, [currentEnv])

  const handleControl = async (slug: string, action: string) => {
    setControlling(slug)
    try {
      await controlApp(currentEnv, slug, action)
      await new Promise((r) => setTimeout(r, 1500))
      const envs = await fetchEnvironments()
      const env = envs.find((e) => e.slug === currentEnv)
      if (env?.apps) {
        const isDev = isDevEnv(env.env_type)
        setApps(env.apps.map((a) => ({
          ...a,
          status: a.running ? 'running' as const : 'stopped' as const,
          url: `https://${a.slug}.${currentEnv}.mynetwk.biz`,
          studio_url: isDev ? `https://studio.${currentEnv}.mynetwk.biz/?folder=/apps/${a.slug}` : undefined,
        })))
      }
    } catch (e) {
      console.warn('Control failed:', e)
    } finally {
      setControlling(null)
    }
  }

  const runningCount = apps.filter((a) => a.running || a.status === 'running').length

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-center">
          <div className="w-6 h-6 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin mx-auto mb-3" />
          <p className="text-white/30 text-sm">Loading...</p>
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-8">
      {/* Welcome Section */}
      <div>
        <h1 className="text-2xl font-bold text-[#e2e8f0]">Welcome back</h1>
        <p className="text-sm text-white/40 mt-1">
          {runningCount} of {apps.length} apps running in current environment
        </p>
      </div>

      {error && (
        <div className="bg-amber-500/10 border border-amber-500/30 rounded-xl p-4">
          <p className="text-sm text-amber-400">API error (showing cached/mock data): {error}</p>
        </div>
      )}

      {/* Quick Actions */}
      <div className="flex items-center gap-3">
        <Link
          to="/pipelines"
          className="px-4 py-2 rounded-lg text-sm font-medium bg-[#7c3aed] text-white hover:bg-[#6d28d9] transition-colors"
        >
          View Pipelines
        </Link>
      </div>

      {/* Apps Table */}
      <section>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-xs font-medium text-white/30 uppercase tracking-wider">
            Applications
          </h2>
          <Link to="/apps" className="text-xs text-[#a78bfa] hover:text-[#c4b5fd]">
            View all
          </Link>
        </div>
        <AppTable
          apps={apps}
          envSlug={currentEnv}
          onControl={currentEnvData && isDevEnv(currentEnvData.env_type) ? handleControl : undefined}
          controlling={controlling}
        />
      </section>

      {/* Recent Pipelines */}
      <section>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-xs font-medium text-white/30 uppercase tracking-wider">
            Recent Pipelines
          </h2>
          <Link to="/pipelines" className="text-xs text-[#a78bfa] hover:text-[#c4b5fd]">
            View all
          </Link>
        </div>
        {pipelines.length === 0 ? (
          <div className="rounded-xl border border-white/5 p-8 text-center" style={{ background: '#1e1e3a' }}>
            <p className="text-sm text-white/30">No recent pipeline runs.</p>
          </div>
        ) : (
          <div className="space-y-2">
            {pipelines.map((p) => (
              <PipelineRow key={p.id} pipeline={p} />
            ))}
          </div>
        )}
      </section>

    </div>
  )
}
