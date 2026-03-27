import { useState, useRef, useEffect } from 'react'
import type { Environment, EnvType } from '../types'

interface EnvSwitcherProps {
  environments: Environment[]
  current: string
  onChange: (slug: string) => void
}

const envTypeStyles: Record<EnvType, { bg: string; text: string; label: string }> = {
  dev: { bg: 'bg-blue-500/20', text: 'text-blue-400', label: 'DEV' },
  acc: { bg: 'bg-amber-500/20', text: 'text-amber-400', label: 'ACC' },
  prod: { bg: 'bg-emerald-500/20', text: 'text-emerald-400', label: 'PROD' },
}

export function EnvSwitcher({ environments, current, onChange }: EnvSwitcherProps) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const currentEnv = environments.find((e) => e.slug === current)

  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [])

  const style = currentEnv ? envTypeStyles[currentEnv.env_type] : null

  return (
    <div ref={ref} className="relative">
      {/* Trigger button */}
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-2.5 px-3 py-1.5 rounded-lg bg-white/10 hover:bg-white/15 border border-white/10 transition-colors min-w-[200px]"
      >
        {/* Status dot */}
        <span className={`w-2 h-2 rounded-full shrink-0 ${currentEnv?.agent_connected ? 'bg-emerald-400' : 'bg-slate-500'}`} />

        {/* Env name */}
        <span className="text-sm font-medium text-white/90 truncate">
          {currentEnv?.name || current}
        </span>

        {/* Type badge */}
        {style && (
          <span className={`px-1.5 py-0.5 rounded text-[10px] font-bold tracking-wider ${style.bg} ${style.text}`}>
            {style.label}
          </span>
        )}

        {/* Chevron */}
        <svg className={`w-3.5 h-3.5 text-white/40 ml-auto transition-transform ${open ? 'rotate-180' : ''}`} fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 8.25l-7.5 7.5-7.5-7.5" />
        </svg>
      </button>

      {/* Dropdown */}
      {open && (
        <div
          className="absolute top-full left-1/2 -translate-x-1/2 mt-1.5 w-72 rounded-xl border border-white/10 shadow-2xl overflow-hidden z-50"
          style={{ background: '#1e1e3a' }}
        >
          <div className="p-2 border-b border-white/10">
            <p className="text-[10px] text-white/30 uppercase tracking-wider px-2 py-1">Select environment</p>
          </div>
          <div className="p-1.5 max-h-64 overflow-auto">
            {environments.map((env) => {
              const s = envTypeStyles[env.env_type]
              const isActive = env.slug === current
              return (
                <button
                  key={env.slug}
                  onClick={() => { onChange(env.slug); setOpen(false) }}
                  className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-left transition-colors ${
                    isActive ? 'bg-[#7c3aed]/20' : 'hover:bg-white/5'
                  }`}
                >
                  {/* Status dot */}
                  <span className={`w-2 h-2 rounded-full shrink-0 ${env.agent_connected ? 'bg-emerald-400' : 'bg-slate-500'}`} />

                  {/* Name + details */}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className={`text-sm font-medium truncate ${isActive ? 'text-[#a78bfa]' : 'text-white/80'}`}>
                        {env.name}
                      </span>
                      <span className={`px-1.5 py-0.5 rounded text-[10px] font-bold tracking-wider ${s.bg} ${s.text}`}>
                        {s.label}
                      </span>
                    </div>
                    <p className="text-[11px] text-white/30 mt-0.5">
                      {env.apps?.length ?? 0} apps
                      {env.agent_version ? ` \u00b7 v${env.agent_version}` : ''}
                    </p>
                  </div>

                  {/* Check mark for selected */}
                  {isActive && (
                    <svg className="w-4 h-4 text-[#7c3aed] shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                    </svg>
                  )}
                </button>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
}
