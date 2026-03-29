import { StatusBadge, StackBadge } from './StatusBadge'
import type { EnvApp } from '../types'
import { Link } from 'react-router-dom'

// ── Inline icons (14x14) ───────────────────────────────────────

function PlayIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
      <path d="M3.5 2.5v9l8-4.5z" />
    </svg>
  )
}

function StopIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
      <rect x="3" y="3" width="8" height="8" rx="1" />
    </svg>
  )
}

function RestartIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M1.5 7a5.5 5.5 0 0 1 9.37-3.9M12.5 7a5.5 5.5 0 0 1-9.37 3.9" />
      <path d="M10.87 1v2.1h-2.1M3.13 13v-2.1h2.1" />
    </svg>
  )
}

function Spinner() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.5" className="animate-spin">
      <path d="M7 1.5a5.5 5.5 0 1 1-5.5 5.5" strokeLinecap="round" />
    </svg>
  )
}

interface AppRowProps {
  app: EnvApp
  envSlug?: string
  onControl?: (slug: string, action: string) => void
  controlling?: string | null
}

export function AppRow({ app, envSlug, onControl, controlling }: AppRowProps) {
  const displayStatus = app.status || (app.running ? 'running' : 'stopped')
  const displayVersion = app.version || '-'

  return (
    <tr className="border-b border-white/5 hover:bg-white/[0.02] transition-colors">
      {/* App name */}
      <td className="px-4 py-3">
        <div className="flex items-center gap-3">
          <span className={`w-2 h-2 rounded-full shrink-0 ${app.running ? 'bg-emerald-400' : 'bg-slate-500'}`} />
          <div>
            <Link
              to={`/apps/${app.slug}`}
              className="text-sm font-medium text-[#e2e8f0] hover:text-[#a78bfa] transition-colors"
            >
              {app.name}
            </Link>
          </div>
        </div>
      </td>

      {/* Stack */}
      <td className="px-4 py-3">
        <StackBadge stack={app.stack} />
      </td>

      {/* Port */}
      <td className="px-4 py-3">
        <span className="text-sm font-mono text-white/40">{app.port || '-'}</span>
      </td>

      {/* Version */}
      <td className="px-4 py-3">
        <span className="text-sm font-mono text-white/50">{displayVersion}</span>
      </td>

      {/* Status */}
      <td className="px-4 py-3">
        <StatusBadge status={displayStatus} />
      </td>

      {/* DB */}
      <td className="px-4 py-3">
        {app.has_db ? (
          envSlug ? (
            <Link
              to={`/environments/${envSlug}/db/${app.slug}`}
              className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-[#7c3aed]/15 text-[#a78bfa] hover:bg-[#7c3aed]/25 transition-colors"
            >
              DB
            </Link>
          ) : (
            <span className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-white/5 text-white/40">
              DB
            </span>
          )
        ) : (
          <span className="text-white/20 text-xs">-</span>
        )}
      </td>

      {/* Actions */}
      <td className="px-4 py-3">
        <div className="flex items-center gap-2 justify-end">
          {onControl && (
            <div className="flex items-center gap-1 mr-2">
              {app.running ? (
                <>
                  <button
                    onClick={() => onControl(app.slug, 'restart')}
                    disabled={controlling === app.slug}
                    className="p-1 rounded hover:bg-white/10 text-blue-400 hover:text-blue-300 transition-colors disabled:opacity-50"
                    title="Restart"
                  >
                    {controlling === app.slug ? <Spinner /> : <RestartIcon />}
                  </button>
                  <button
                    onClick={() => onControl(app.slug, 'stop')}
                    disabled={controlling === app.slug}
                    className="p-1 rounded hover:bg-white/10 text-red-400 hover:text-red-300 transition-colors disabled:opacity-50"
                    title="Stop"
                  >
                    <StopIcon />
                  </button>
                </>
              ) : (
                <button
                  onClick={() => onControl(app.slug, 'start')}
                  disabled={controlling === app.slug}
                  className="p-1 rounded hover:bg-white/10 text-emerald-400 hover:text-emerald-300 transition-colors disabled:opacity-50"
                  title="Start"
                >
                  {controlling === app.slug ? <Spinner /> : <PlayIcon />}
                </button>
              )}
            </div>
          )}
          {app.url && (
            <a
              href={app.url}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium bg-white/5 text-white/60 hover:bg-white/10 hover:text-white/80 border border-white/10 transition-colors"
            >
              <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 6H5.25A2.25 2.25 0 003 8.25v10.5A2.25 2.25 0 005.25 21h10.5A2.25 2.25 0 0018 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" />
              </svg>
              Open
            </a>
          )}
          {app.studio_url && (
            <a
              href={app.studio_url}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium bg-[#7c3aed]/15 text-[#a78bfa] hover:bg-[#7c3aed]/25 border border-[#7c3aed]/20 transition-colors"
            >
              <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M17.25 6.75L22.5 12l-5.25 5.25m-10.5 0L1.5 12l5.25-5.25m7.5-3l-4.5 16.5" />
              </svg>
              Studio
            </a>
          )}
        </div>
      </td>
    </tr>
  )
}

/** Reusable table wrapper for app lists */
export function AppTable({ apps, envSlug, onControl, controlling }: {
  apps: EnvApp[]
  envSlug?: string
  onControl?: (slug: string, action: string) => void
  controlling?: string | null
}) {
  if (apps.length === 0) {
    return (
      <div className="rounded-xl border border-white/5 p-8 text-center" style={{ background: '#1e1e3a' }}>
        <p className="text-sm text-white/30">No applications found.</p>
      </div>
    )
  }

  return (
    <div className="rounded-xl border border-white/5 overflow-hidden" style={{ background: '#1e1e3a' }}>
      <table className="w-full">
        <thead>
          <tr className="border-b border-white/10">
            <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Name</th>
            <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Stack</th>
            <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Port</th>
            <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Version</th>
            <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Status</th>
            <th className="text-left px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">DB</th>
            <th className="text-right px-4 py-2.5 text-[11px] font-medium text-white/30 uppercase tracking-wider">Actions</th>
          </tr>
        </thead>
        <tbody>
          {apps.map((app) => (
            <AppRow key={app.slug} app={app} envSlug={envSlug} onControl={onControl} controlling={controlling} />
          ))}
        </tbody>
      </table>
    </div>
  )
}
