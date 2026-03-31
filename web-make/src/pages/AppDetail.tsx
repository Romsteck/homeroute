import { useEffect, useState } from 'react'
import { useParams, Link, useNavigate } from 'react-router-dom'
import { StatusBadge, EnvTypeBadge, StackBadge } from '../components/StatusBadge'
import { PipelineRow } from '../components/PipelineRow'
import { fetchAppDetail, triggerPipeline, fetchPipelines, toggleAppAuth } from '../api'
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
  const [togglingAuth, setTogglingAuth] = useState<string | null>(null)

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

  const isProdEnv = (envType: string) => envType === 'prod' || envType === 'production'

  const handleToggleAuth = async (envSlug: string, currentPublic: boolean) => {
    if (!slug) return
    setTogglingAuth(envSlug)
    try {
      await toggleAppAuth(envSlug, slug, !currentPublic)
      // Refresh app data
      const refreshed = await fetchAppDetail(slug)
      setApp(refreshed)
    } catch (e) {
      alert('Failed to toggle auth: ' + (e as Error).message)
    } finally {
      setTogglingAuth(null)
    }
  }

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
                <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Access</th>
                <th className="text-right px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Actions</th>
              </tr>
            </thead>
            <tbody>
              {app.environments.map((env) => (
                <tr key={env.env_slug} className="border-b border-white/5 last:border-0 hover:bg-white/[0.02] transition-colors">
                  <td className="px-4 py-3">
                    <span className="text-sm text-[#e2e8f0]">
                      {env.env_name}
                    </span>
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
                  <td className="px-4 py-3">
                    {isProdEnv(env.env_type) ? (
                      <button
                        onClick={() => handleToggleAuth(env.env_slug, !!env.public)}
                        disabled={togglingAuth === env.env_slug}
                        className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium border transition-colors ${
                          env.public
                            ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20 hover:bg-emerald-500/20'
                            : 'bg-amber-500/10 text-amber-400 border-amber-500/20 hover:bg-amber-500/20'
                        } disabled:opacity-50`}
                        title={env.public ? 'Click to require authentication' : 'Click to make public'}
                      >
                        {togglingAuth === env.env_slug ? (
                          <span className="w-3 h-3 border border-current border-t-transparent rounded-full animate-spin" />
                        ) : env.public ? (
                          <svg xmlns="http://www.w3.org/2000/svg" className="w-3 h-3" viewBox="0 0 20 20" fill="currentColor"><path fillRule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM4.332 8.027a6.012 6.012 0 011.912-2.706C6.512 5.73 6.974 6 7.5 6A1.5 1.5 0 019 7.5V8a2 2 0 004 0 2 2 0 012 2v1a2 2 0 01-2 2 2 2 0 00-2 2 2 2 0 01-2 2h-.5a6.018 6.018 0 01-4.166-5.973z" clipRule="evenodd" /></svg>
                        ) : (
                          <svg xmlns="http://www.w3.org/2000/svg" className="w-3 h-3" viewBox="0 0 20 20" fill="currentColor"><path fillRule="evenodd" d="M5 9V7a5 5 0 0110 0v2a2 2 0 012 2v5a2 2 0 01-2 2H5a2 2 0 01-2-2v-5a2 2 0 012-2zm8-2v2H7V7a3 3 0 016 0z" clipRule="evenodd" /></svg>
                        )}
                        {env.public ? 'Public' : 'Auth'}
                      </button>
                    ) : (
                      <span className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs text-white/30">
                        <svg xmlns="http://www.w3.org/2000/svg" className="w-3 h-3" viewBox="0 0 20 20" fill="currentColor"><path fillRule="evenodd" d="M5 9V7a5 5 0 0110 0v2a2 2 0 012 2v5a2 2 0 01-2 2H5a2 2 0 01-2-2v-5a2 2 0 012-2zm8-2v2H7V7a3 3 0 016 0z" clipRule="evenodd" /></svg>
                        Auth (forced)
                      </span>
                    )}
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
