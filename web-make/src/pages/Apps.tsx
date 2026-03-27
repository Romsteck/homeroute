import { useEffect, useState } from 'react'
import { AppTable } from '../components/AppRow'
import { fetchApps } from '../api'
import type { EnvApp } from '../types'

interface AppsProps {
  currentEnv: string
}

export function Apps({ currentEnv }: AppsProps) {
  const [apps, setApps] = useState<EnvApp[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setLoading(true)
    setError(null)
    fetchApps(currentEnv)
      .then(setApps)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false))
  }, [currentEnv])

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-center">
          <div className="w-6 h-6 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin mx-auto mb-3" />
          <p className="text-white/30 text-sm">Loading apps...</p>
        </div>
      </div>
    )
  }

  const runningCount = apps.filter((a) => a.running || a.status === 'running').length

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-bold text-[#e2e8f0]">Apps</h1>
        <p className="text-sm text-white/40 mt-1">
          {apps.length} apps in current environment, {runningCount} running
        </p>
      </div>

      {error && (
        <div className="bg-amber-500/10 border border-amber-500/30 rounded-xl p-4">
          <p className="text-sm text-amber-400">API error (showing cached/mock data): {error}</p>
        </div>
      )}

      <AppTable apps={apps} envSlug={currentEnv} />
    </div>
  )
}
