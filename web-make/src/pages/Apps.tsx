import { useEffect, useState } from 'react'
import { AppTable } from '../components/AppRow'
import { CreateAppModal } from '../components/CreateAppModal'
import { fetchApps, fetchEnvironments, controlApp } from '../api'
import type { EnvApp } from '../types'
import { isDevEnv } from '../types'

interface AppsProps {
  currentEnv: string
}

export function Apps({ currentEnv }: AppsProps) {
  const [apps, setApps] = useState<EnvApp[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [controlling, setControlling] = useState<string | null>(null)
  const [isDev, setIsDev] = useState(true)
  const [showCreateApp, setShowCreateApp] = useState(false)

  useEffect(() => {
    setLoading(true)
    setError(null)
    Promise.all([fetchApps(currentEnv), fetchEnvironments()])
      .then(([a, envs]) => {
        setApps(a)
        const env = envs.find((e) => e.slug === currentEnv)
        setIsDev(env ? isDevEnv(env.env_type) : false)
      })
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

  const handleControl = async (slug: string, action: string) => {
    setControlling(slug)
    try {
      await controlApp(currentEnv, slug, action)
      await new Promise((r) => setTimeout(r, 1500))
      const refreshed = await fetchApps(currentEnv)
      setApps(refreshed)
    } catch (e) {
      console.warn('Control failed:', e)
    } finally {
      setControlling(null)
    }
  }

  const runningCount = apps.filter((a) => a.running || a.status === 'running').length

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-[#e2e8f0]">Apps</h1>
          <p className="text-sm text-white/40 mt-1">
            {apps.length} apps in current environment, {runningCount} running
          </p>
        </div>
        {isDev && (
          <button
            onClick={() => setShowCreateApp(true)}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium bg-[#7c3aed]/15 text-[#a78bfa] hover:bg-[#7c3aed]/25 border border-[#7c3aed]/20 transition-colors"
          >
            <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
            </svg>
            Create App
          </button>
        )}
      </div>

      {error && (
        <div className="bg-amber-500/10 border border-amber-500/30 rounded-xl p-4">
          <p className="text-sm text-amber-400">API error (showing cached/mock data): {error}</p>
        </div>
      )}

      <AppTable apps={apps} envSlug={currentEnv} onControl={isDev ? handleControl : undefined} controlling={controlling} />

      {showCreateApp && (
        <CreateAppModal
          envSlug={currentEnv}
          onCreated={() => {
            setShowCreateApp(false)
            fetchApps(currentEnv).then(setApps).catch(() => {})
          }}
          onClose={() => setShowCreateApp(false)}
        />
      )}
    </div>
  )
}
