import { useEffect, useState } from 'react'
import { useParams, Link } from 'react-router-dom'
import { AppTable } from '../components/AppRow'
import { StatusBadge, EnvTypeBadge } from '../components/StatusBadge'
import { fetchEnvironment, fetchEnvironments, fetchApps, fetchDbTables, fetchPipelines } from '../api'
import type { Environment, EnvApp, DbTable, PipelineRun } from '../types'
import { PipelineRow } from '../components/PipelineRow'

const BASE_DOMAIN = 'mynetwk.biz'

export function EnvironmentDetail() {
  const { slug } = useParams<{ slug: string }>()
  const [env, setEnv] = useState<Environment | null>(null)
  const [apps, setApps] = useState<EnvApp[]>([])
  const [dbTables, setDbTables] = useState<DbTable[]>([])
  const [pipelines, setPipelines] = useState<PipelineRun[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [activeTab, setActiveTab] = useState<'apps' | 'tables' | 'pipelines' | 'settings'>('apps')

  useEffect(() => {
    if (!slug) return
    setLoading(true)
    setError(null)

    // Fetch environment info
    fetchEnvironment(slug)
      .then((env) => {
        if (env) {
          setEnv(env)
          return
        }
        return fetchEnvironments().then((envs) => {
          setEnv(envs.find((e) => e.slug === slug) ?? null)
        })
      })
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false))

    // Fetch apps
    fetchApps(slug).then(setApps).catch(() => {})

    // Fetch DB tables (best-effort)
    fetchDbTables(slug).then(setDbTables).catch(() => setDbTables([]))

    // Fetch pipelines for this env
    fetchPipelines().then((pipes) => {
      setPipelines(pipes.filter((p) => p.source_env === slug || p.target_env === slug))
    }).catch(() => setPipelines([]))
  }, [slug])

  const displayApps = apps.length > 0 ? apps : (env?.apps || []).map((a) => ({
    ...a,
    status: (a.running ? 'running' : 'stopped') as EnvApp['status'],
    url: `https://${a.slug}.${slug}.${BASE_DOMAIN}`,
    studio_url: `https://studio.${slug}.${BASE_DOMAIN}/?folder=/apps/${a.slug}`,
  }))

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

  if (!env) {
    return (
      <div className="flex items-center justify-center h-64">
        <p className="text-white/30">Environment not found.</p>
      </div>
    )
  }

  const tabs = [
    { key: 'apps' as const, label: 'Apps', count: displayApps.length },
    { key: 'tables' as const, label: 'Tables', count: dbTables.length },
    { key: 'pipelines' as const, label: 'Pipelines', count: pipelines.length },
    { key: 'settings' as const, label: 'Settings', count: null },
  ]

  return (
    <div className="space-y-6">
      {/* Breadcrumb + Header */}
      <div>
        <div className="flex items-center gap-2 mb-2">
          <Link to="/environments" className="text-xs text-white/30 hover:text-white/50">
            Environments
          </Link>
          <span className="text-xs text-white/15">/</span>
          <span className="text-xs text-white/50">{env.name}</span>
        </div>
        <div className="flex items-center gap-3">
          <span className={`w-2.5 h-2.5 rounded-full ${env.agent_connected ? 'bg-emerald-400' : 'bg-white/20'}`} />
          <h1 className="text-2xl font-bold text-[#e2e8f0]">{env.name}</h1>
          <EnvTypeBadge envType={env.env_type} />
          <StatusBadge status={env.status} />
        </div>
        <div className="flex items-center gap-4 mt-2 text-xs text-white/30">
          {env.agent_connected ? (
            <span className="text-emerald-400">Agent v{env.agent_version || '?'}</span>
          ) : (
            <span>Agent offline</span>
          )}
          <span>{env.ipv4_address || 'No IP'}</span>
          <span>{env.host_id}</span>
          <span>{env.container_name}</span>
        </div>
      </div>

      {error && (
        <div className="bg-amber-500/10 border border-amber-500/30 rounded-xl p-4">
          <p className="text-sm text-amber-400">API error: {error}</p>
        </div>
      )}

      {/* Quick Actions */}
      <div className="flex items-center gap-3">
        <a
          href={`https://studio.${slug}.${BASE_DOMAIN}`}
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium bg-[#7c3aed]/15 text-[#a78bfa] hover:bg-[#7c3aed]/25 border border-[#7c3aed]/20 transition-colors"
        >
          <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M17.25 6.75L22.5 12l-5.25 5.25m-10.5 0L1.5 12l5.25-5.25m7.5-3l-4.5 16.5" />
          </svg>
          Open Studio
        </a>
        <Link
          to={`/environments/${slug}/db`}
          className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium bg-white/5 text-white/60 hover:bg-white/10 border border-white/10 transition-colors"
        >
          <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M20.25 6.375c0 2.278-3.694 4.125-8.25 4.125S3.75 8.653 3.75 6.375m16.5 0c0-2.278-3.694-4.125-8.25-4.125S3.75 4.097 3.75 6.375m16.5 0v11.25c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125V6.375m16.5 0v3.75m-16.5-3.75v3.75m16.5 0v3.75C20.25 16.153 16.556 18 12 18s-8.25-1.847-8.25-4.125v-3.75m16.5 0c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125" />
          </svg>
          DB Explorer
        </Link>
      </div>

      {/* Tabs */}
      <div className="flex items-center gap-1 border-b border-white/10">
        {tabs.map((tab) => (
          <button
            key={tab.key}
            onClick={() => setActiveTab(tab.key)}
            className={`px-4 py-2.5 text-sm font-medium transition-colors relative ${
              activeTab === tab.key
                ? 'text-[#a78bfa]'
                : 'text-white/40 hover:text-white/60'
            }`}
          >
            {tab.label}
            {tab.count !== null && (
              <span className="ml-1.5 text-[10px] text-white/20">{tab.count}</span>
            )}
            {activeTab === tab.key && (
              <span className="absolute bottom-0 left-0 right-0 h-0.5 bg-[#7c3aed] rounded-t" />
            )}
          </button>
        ))}
      </div>

      {/* Tab Content */}
      {activeTab === 'apps' && (
        <AppTable apps={displayApps} envSlug={slug} />
      )}

      {activeTab === 'tables' && (
        <div>
          {dbTables.length === 0 ? (
            <div className="rounded-xl border border-white/5 p-8 text-center" style={{ background: '#1e1e3a' }}>
              <p className="text-sm text-white/30">No database tables found in this environment.</p>
            </div>
          ) : (
            <div className="rounded-xl border border-white/5 overflow-hidden" style={{ background: '#1e1e3a' }}>
              <table className="w-full">
                <thead>
                  <tr className="border-b border-white/10">
                    <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Table</th>
                    <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Rows</th>
                    <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Columns</th>
                    <th className="text-right px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {dbTables.map((t) => (
                    <tr key={t.name} className="border-b border-white/5 hover:bg-white/[0.02] transition-colors">
                      <td className="px-4 py-3">
                        <span className="text-sm font-mono text-[#e2e8f0]">{t.name}</span>
                      </td>
                      <td className="px-4 py-3">
                        <span className="text-sm text-white/50">{t.row_count.toLocaleString()}</span>
                      </td>
                      <td className="px-4 py-3">
                        <span className="text-sm text-white/50">{t.column_count}</span>
                      </td>
                      <td className="px-4 py-3 text-right">
                        <Link
                          to={`/environments/${slug}/db?table=${t.name}`}
                          className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-xs font-medium bg-white/5 text-white/60 hover:bg-white/10 border border-white/10 transition-colors"
                        >
                          Explore
                        </Link>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {activeTab === 'pipelines' && (
        <div>
          {pipelines.length === 0 ? (
            <div className="rounded-xl border border-white/5 p-8 text-center" style={{ background: '#1e1e3a' }}>
              <p className="text-sm text-white/30">No pipeline runs for this environment.</p>
            </div>
          ) : (
            <div className="space-y-2">
              {pipelines.map((p) => (
                <PipelineRow key={p.id} pipeline={p} />
              ))}
            </div>
          )}
        </div>
      )}

      {activeTab === 'settings' && (
        <div className="space-y-4">
          {/* Info grid */}
          <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
            {[
              { label: 'IP Address', value: env.ipv4_address || 'N/A', mono: true },
              { label: 'Host', value: env.host_id, mono: false },
              { label: 'Container', value: env.container_name, mono: true },
              {
                label: 'Created',
                value: env.created_at
                  ? new Date(env.created_at).toLocaleDateString('en-US', { year: 'numeric', month: 'short', day: 'numeric' })
                  : 'N/A',
                mono: false,
              },
            ].map((item) => (
              <div key={item.label} className="p-3 rounded-lg border border-white/5" style={{ background: '#1e1e3a' }}>
                <p className="text-[10px] text-white/30 uppercase tracking-wider mb-1">{item.label}</p>
                <p className={`text-sm text-[#e2e8f0] ${item.mono ? 'font-mono' : ''}`}>{item.value}</p>
              </div>
            ))}
          </div>

          <div className="rounded-xl border border-white/5 p-8 text-center" style={{ background: '#1e1e3a' }}>
            <p className="text-sm text-white/30">
              Environment variables and advanced settings editor coming soon.
            </p>
          </div>
        </div>
      )}
    </div>
  )
}
