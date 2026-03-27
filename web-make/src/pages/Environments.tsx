import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { StatusBadge, EnvTypeBadge } from '../components/StatusBadge'
import { fetchEnvironments } from '../api'
import type { Environment } from '../types'

export function Environments() {
  const [environments, setEnvironments] = useState<Environment[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setLoading(true)
    setError(null)
    fetchEnvironments()
      .then(setEnvironments)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false))
  }, [])

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-center">
          <div className="w-6 h-6 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin mx-auto mb-3" />
          <p className="text-white/30 text-sm">Loading environments...</p>
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-[#e2e8f0]">Environments</h1>
          <p className="text-sm text-white/40 mt-1">{environments.length} environments configured</p>
        </div>
        <button className="px-4 py-2 bg-[#7c3aed] hover:bg-[#6d28d9] text-white text-sm font-medium rounded-lg transition-colors">
          Create Environment
        </button>
      </div>

      {error && (
        <div className="bg-amber-500/10 border border-amber-500/30 rounded-xl p-4">
          <p className="text-sm text-amber-400">API error (showing cached/mock data): {error}</p>
        </div>
      )}

      {/* Environment Table */}
      <div className="rounded-xl border border-white/5 overflow-hidden" style={{ background: '#1e1e3a' }}>
        <table className="w-full">
          <thead>
            <tr className="border-b border-white/10">
              <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Name</th>
              <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Type</th>
              <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Status</th>
              <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Agent</th>
              <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">IP</th>
              <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Apps</th>
              <th className="text-right px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Actions</th>
            </tr>
          </thead>
          <tbody>
            {environments.map((env) => (
              <tr key={env.slug} className="border-b border-white/5 hover:bg-white/[0.02] transition-colors">
                <td className="px-4 py-3">
                  <Link
                    to={`/environments/${env.slug}`}
                    className="flex items-center gap-2.5"
                  >
                    <span className={`w-2 h-2 rounded-full shrink-0 ${env.agent_connected ? 'bg-emerald-400' : 'bg-white/20'}`} />
                    <span className="text-sm font-medium text-[#e2e8f0] hover:text-[#a78bfa] transition-colors">
                      {env.name}
                    </span>
                  </Link>
                </td>
                <td className="px-4 py-3">
                  <EnvTypeBadge envType={env.env_type} />
                </td>
                <td className="px-4 py-3">
                  <StatusBadge status={env.status} />
                </td>
                <td className="px-4 py-3">
                  {env.agent_connected ? (
                    <span className="text-xs text-emerald-400">
                      v{env.agent_version || '?'}
                    </span>
                  ) : (
                    <span className="text-xs text-white/20">offline</span>
                  )}
                </td>
                <td className="px-4 py-3">
                  <span className="text-xs font-mono text-white/40">{env.ipv4_address || '-'}</span>
                </td>
                <td className="px-4 py-3">
                  <span className="text-sm text-white/50">{env.apps?.length ?? 0}</span>
                </td>
                <td className="px-4 py-3 text-right">
                  <Link
                    to={`/environments/${env.slug}`}
                    className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-xs font-medium bg-white/5 text-white/60 hover:bg-white/10 border border-white/10 transition-colors"
                  >
                    View
                  </Link>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
