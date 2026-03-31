import { useEffect, useState } from 'react'
import { useParams, Link } from 'react-router-dom'
import { fetchPipelineConfig, savePipelineConfig, fetchEnvironments } from '../api'
import type { PipelineConfig, Environment } from '../types'

export function PipelineConfigPage() {
  const { slug } = useParams<{ slug: string }>()
  const [config, setConfig] = useState<PipelineConfig | null>(null)
  const [envs, setEnvs] = useState<Environment[]>([])
  const [saving, setSaving] = useState(false)
  const [saved, setSaved] = useState(false)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    if (!slug) return
    setLoading(true)
    Promise.all([fetchPipelineConfig(slug), fetchEnvironments()])
      .then(([cfg, environments]) => {
        setConfig(
          cfg || {
            app_slug: slug,
            env_chain: ['dev'],
            skip_steps: [],
            auto_promote: [],
            gates: [],
          },
        )
        setEnvs(environments)
      })
      .finally(() => setLoading(false))
  }, [slug])

  const handleSave = async () => {
    if (!config) return
    setSaving(true)
    try {
      await savePipelineConfig(config)
      setSaved(true)
      setTimeout(() => setSaved(false), 2000)
    } catch (e) {
      alert('Failed to save: ' + (e as Error).message)
    } finally {
      setSaving(false)
    }
  }

  const toggleSkipStep = (step: string) => {
    if (!config) return
    const skip = new Set(config.skip_steps)
    if (skip.has(step)) skip.delete(step)
    else skip.add(step)
    setConfig({ ...config, skip_steps: [...skip] })
  }

  const toggleAutoPromote = (env: string) => {
    if (!config) return
    const auto = new Set(config.auto_promote)
    if (auto.has(env)) auto.delete(env)
    else auto.add(env)
    setConfig({ ...config, auto_promote: [...auto] })
  }

  const toggleGate = (from: string, to: string) => {
    if (!config) return
    const hasGate = config.gates.some((g) => g.from_env === from && g.to_env === to)
    if (hasGate) {
      setConfig({
        ...config,
        gates: config.gates.filter((g) => !(g.from_env === from && g.to_env === to)),
      })
    } else {
      setConfig({ ...config, gates: [...config.gates, { from_env: from, to_env: to }] })
    }
  }

  const addEnvToChain = (env: string) => {
    if (!config || config.env_chain.includes(env)) return
    setConfig({ ...config, env_chain: [...config.env_chain, env] })
  }

  const removeEnvFromChain = (env: string) => {
    if (!config) return
    setConfig({
      ...config,
      env_chain: config.env_chain.filter((e) => e !== env),
      gates: config.gates.filter((g) => g.from_env !== env && g.to_env !== env),
      auto_promote: config.auto_promote.filter((e) => e !== env),
    })
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="w-6 h-6 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin" />
      </div>
    )
  }

  if (!config) return null

  const availableEnvs = envs.filter((e) => !config.env_chain.includes(e.slug))
  const optionalSteps = ['test', 'backup-db']

  return (
    <div className="space-y-6 max-w-2xl mx-auto">
      {/* Breadcrumb */}
      <div className="flex items-center gap-2">
        <Link to={`/apps/${slug}`} className="text-white/30 hover:text-white/60 text-sm">
          &larr; {slug}
        </Link>
      </div>

      <div>
        <h1 className="text-2xl font-bold text-[#e2e8f0]">Pipeline Configuration</h1>
        <p className="text-sm text-white/40 mt-1">{slug}</p>
      </div>

      {/* Environment Chain */}
      <div className="rounded-xl border border-white/5 p-6" style={{ background: '#1e1e3a' }}>
        <h2 className="text-sm font-medium text-white/50 mb-4">Promotion Chain</h2>
        <div className="flex items-center gap-2 flex-wrap">
          {config.env_chain.map((env, i) => (
            <div key={env} className="flex items-center gap-2">
              <div className="px-3 py-1.5 rounded-lg bg-[#7c3aed]/20 border border-[#7c3aed]/30 text-sm text-[#c4b5fd] flex items-center gap-2">
                {env}
                {config.env_chain.length > 1 && (
                  <button
                    onClick={() => removeEnvFromChain(env)}
                    className="text-white/20 hover:text-red-400 text-xs"
                  >
                    &times;
                  </button>
                )}
              </div>
              {i < config.env_chain.length - 1 && <span className="text-white/20">&rarr;</span>}
            </div>
          ))}
          {availableEnvs.length > 0 && (
            <select
              onChange={(e) => {
                if (e.target.value) addEnvToChain(e.target.value)
                e.target.value = ''
              }}
              className="rounded-lg px-2 py-1 text-sm text-white/40 border border-white/10 cursor-pointer"
              style={{ background: '#0f0f23' }}
              defaultValue=""
            >
              <option value="">+ Add env</option>
              {availableEnvs.map((e) => (
                <option key={e.slug} value={e.slug}>
                  {e.slug}
                </option>
              ))}
            </select>
          )}
        </div>
      </div>

      {/* Optional Steps */}
      <div className="rounded-xl border border-white/5 p-6" style={{ background: '#1e1e3a' }}>
        <h2 className="text-sm font-medium text-white/50 mb-4">Optional Steps</h2>
        <div className="space-y-3">
          {optionalSteps.map((step) => (
            <label key={step} className="flex items-center gap-3 cursor-pointer">
              <input
                type="checkbox"
                checked={!config.skip_steps.includes(step)}
                onChange={() => toggleSkipStep(step)}
                className="rounded border-white/20 bg-transparent text-[#7c3aed] focus:ring-[#7c3aed]"
              />
              <span className="text-sm text-white/60">{step}</span>
            </label>
          ))}
        </div>
      </div>

      {/* Gates & Auto-promote */}
      {config.env_chain.length > 1 && (
        <div className="rounded-xl border border-white/5 p-6" style={{ background: '#1e1e3a' }}>
          <h2 className="text-sm font-medium text-white/50 mb-4">Transitions</h2>
          <div className="space-y-3">
            {config.env_chain.slice(0, -1).map((env, i) => {
              const nextEnv = config.env_chain[i + 1]
              const hasGate = config.gates.some((g) => g.from_env === env && g.to_env === nextEnv)
              const isAuto = config.auto_promote.includes(env)
              return (
                <div
                  key={`${env}-${nextEnv}`}
                  className="flex items-center justify-between p-3 rounded-lg bg-white/5"
                >
                  <span className="text-sm text-white/60">
                    {env} &rarr; {nextEnv}
                  </span>
                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={isAuto}
                        onChange={() => toggleAutoPromote(env)}
                        className="rounded border-white/20 bg-transparent text-[#7c3aed] focus:ring-[#7c3aed]"
                      />
                      <span className="text-xs text-white/40">Auto</span>
                    </label>
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={hasGate}
                        onChange={() => toggleGate(env, nextEnv)}
                        className="rounded border-white/20 bg-transparent text-amber-500 focus:ring-amber-500"
                      />
                      <span className="text-xs text-white/40">Gate</span>
                    </label>
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      )}

      {/* Save */}
      <div className="flex justify-end">
        <button
          onClick={handleSave}
          disabled={saving}
          className={`px-6 py-2 text-sm font-medium rounded-lg transition-colors ${
            saved
              ? 'bg-emerald-600 text-white'
              : 'bg-[#7c3aed] hover:bg-[#6d28d9] text-white'
          }`}
        >
          {saving ? 'Saving...' : saved ? 'Saved!' : 'Save Configuration'}
        </button>
      </div>
    </div>
  )
}
