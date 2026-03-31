import { useEffect, useState } from 'react'
import { useParams, Link, useNavigate } from 'react-router-dom'
import { StatusBadge, EnvTypeBadge, StackBadge } from '../components/StatusBadge'
import { PipelineRow } from '../components/PipelineRow'
import { fetchAppDetail, triggerPipeline, fetchPipelines } from '../api'
import type { AppInfo, PipelineRun } from '../types'
import { isDevEnv } from '../types'

const BASE_DOMAIN = 'mynetwk.biz'

export function AppDetail() {
  const { slug } = useParams<{ slug: string }>()
  const navigate = useNavigate()
  const [app, setApp] = useState<AppInfo | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [recentPipelines, setRecentPipelines] = useState<PipelineRun[]>([])
  const [promoting, setPromoting] = useState<string | null>(null)

  useEffect(() => {
    if (slug) {
      setLoading(true)
      setError(null)
      fetchAppDetail(slug)
        .then(setApp)
        .catch((e) => setError(e.message))
        .finally(() => setLoading(false))

      fetchPipelines(20)
        .then((pipes) => setRecentPipelines(pipes.filter((p) => p.app_slug === slug).slice(0, 5)))
        .catch(() => {})
    }
  }, [slug])

  const handlePromote = async (sourceEnv: string, targetEnv: string) => {
    if (!slug) return
    setPromoting(sourceEnv)
    try {
      const result = await triggerPipeline(slug, sourceEnv, targetEnv)
      if (result.id && result.id !== 'unknown') {
        navigate(`/pipelines/${result.id}`)
      }
    } catch (e) {
      alert('Failed to promote: ' + (e as Error).message)
    } finally {
      setPromoting(null)
    }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-center">
          <div className="w-6 h-6 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin mx-auto mb-3" />
          <p className="text-white/30 text-sm">Loading application...</p>
        </div>
      </div>
    )
  }

  if (!app) {
    return (
      <div className="flex items-center justify-center h-64">
        <p className="text-white/30">Application not found.</p>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <div className="flex items-center gap-2 mb-2">
          <Link to="/" className="text-xs text-white/30 hover:text-white/50">
            Home
          </Link>
          <span className="text-xs text-white/15">/</span>
          <Link to="/apps" className="text-xs text-white/30 hover:text-white/50">
            Apps
          </Link>
          <span className="text-xs text-white/15">/</span>
          <span className="text-xs text-white/50">{app.name}</span>
        </div>
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-bold text-[#e2e8f0]">{app.name}</h1>
          <StackBadge stack={app.stack} />
          <Link
            to={`/apps/${slug}/pipeline`}
            className="ml-auto px-3 py-1.5 rounded-md text-xs font-medium bg-white/5 text-white/50 hover:bg-white/10 border border-white/10 transition-colors"
          >
            Pipeline Config
          </Link>
        </div>
      </div>

      {error && (
        <div className="bg-amber-500/10 border border-amber-500/30 rounded-xl p-4">
          <p className="text-sm text-amber-400">API error: {error}</p>
        </div>
      )}

      {/* Version Comparison Table */}
      <section>
        <h2 className="text-xs font-medium text-white/30 uppercase tracking-wider mb-3">
          Environments
        </h2>
        <div className="rounded-xl border border-white/5 overflow-hidden" style={{ background: '#1e1e3a' }}>
          <table className="w-full">
            <thead>
              <tr className="border-b border-white/10">
                <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Environment</th>
                <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Type</th>
                <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Version</th>
                <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Status</th>
                <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Last Deploy</th>
                <th className="text-right px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Actions</th>
              </tr>
            </thead>
            <tbody>
              {app.environments.map((env) => (
                <tr key={env.env_slug} className="border-b border-white/5 last:border-0 hover:bg-white/[0.02] transition-colors">
                  <td className="px-4 py-3">
                    <Link
                      to={`/environments/${env.env_slug}`}
                      className="text-sm text-[#e2e8f0] hover:text-[#a78bfa] transition-colors"
                    >
                      {env.env_name}
                    </Link>
                  </td>
                  <td className="px-4 py-3">
                    <EnvTypeBadge envType={env.env_type} />
                  </td>
                  <td className="px-4 py-3">
                    <span className="text-sm font-mono text-white/50">{env.version}</span>
                  </td>
                  <td className="px-4 py-3">
                    <StatusBadge status={env.status} />
                  </td>
                  <td className="px-4 py-3">
                    <span className="text-xs text-white/30">
                      {env.last_deploy
                        ? new Date(env.last_deploy).toLocaleDateString('en-US', {
                            month: 'short',
                            day: 'numeric',
                            hour: '2-digit',
                            minute: '2-digit',
                          })
                        : '-'}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-right">
                    <div className="flex items-center gap-2 justify-end">
                      <a
                        href={`https://${app.slug}.${env.env_slug}.${BASE_DOMAIN}`}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-xs font-medium bg-white/5 text-white/60 hover:bg-white/10 border border-white/10 transition-colors"
                      >
                        Open
                      </a>
                      {isDevEnv(env.env_type) && (
                        <>
                          <a
                            href={`https://studio.${env.env_slug}.${BASE_DOMAIN}/?folder=/apps/${app.slug}`}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-xs font-medium bg-[#7c3aed]/15 text-[#a78bfa] hover:bg-[#7c3aed]/25 border border-[#7c3aed]/20 transition-colors"
                          >
                            Studio
                          </a>
                          {(() => {
                            // Find next env in the list to promote to
                            const envIndex = app.environments.findIndex((e) => e.env_slug === env.env_slug)
                            const nextEnv = app.environments[envIndex + 1]
                            if (!nextEnv) return null
                            return (
                              <button
                                onClick={() => handlePromote(env.env_slug, nextEnv.env_slug)}
                                disabled={promoting === env.env_slug}
                                className="px-2.5 py-1 rounded-md text-xs font-medium bg-[#7c3aed]/15 text-[#a78bfa] hover:bg-[#7c3aed]/25 border border-[#7c3aed]/20 transition-colors disabled:opacity-50"
                              >
                                {promoting === env.env_slug ? 'Promoting...' : `Promote to ${nextEnv.env_slug}`}
                              </button>
                            )
                          })()}
                        </>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      {/* Recent Pipelines */}
      <section>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-xs font-medium text-white/30 uppercase tracking-wider">
            Recent Pipelines
          </h2>
          <Link to="/pipelines" className="text-xs text-[#7c3aed] hover:underline">
            View all
          </Link>
        </div>
        {recentPipelines.length === 0 ? (
          <div className="rounded-xl border border-white/5 p-8 text-center" style={{ background: '#1e1e3a' }}>
            <p className="text-sm text-white/30">No pipeline runs yet for this app.</p>
          </div>
        ) : (
          <div className="space-y-2">
            {recentPipelines.map((p) => (
              <PipelineRow key={p.id} pipeline={p} />
            ))}
          </div>
        )}
      </section>
    </div>
  )
}
