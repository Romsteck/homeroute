import { StatusBadge, StackBadge } from './StatusBadge'
import type { EnvApp } from '../types'
import { Link } from 'react-router-dom'

interface AppCardProps {
  app: EnvApp
}

export function AppCard({ app }: AppCardProps) {
  const displayStatus = app.status || (app.running ? 'running' : 'stopped')
  const displayVersion = app.version || 'N/A'

  return (
    <div className="bg-slate-800/50 border border-slate-700/50 rounded-xl p-4 hover:border-slate-600 transition-colors">
      <div className="flex items-start justify-between mb-3">
        <div>
          <Link
            to={`/apps/${app.slug}`}
            className="text-sm font-semibold text-slate-100 hover:text-indigo-400 transition-colors"
          >
            {app.name}
          </Link>
          <p className="text-xs text-slate-500 mt-0.5">v{displayVersion}</p>
        </div>
        <StatusBadge status={displayStatus} />
      </div>

      <div className="mb-4 flex items-center gap-2">
        <StackBadge stack={app.stack} />
        {app.has_db && (
          <span className="inline-flex items-center px-2 py-0.5 rounded bg-slate-700/50 text-slate-400 text-xs font-medium">
            DB
          </span>
        )}
      </div>

      <div className="flex items-center gap-2">
        {app.url && (
          <a
            href={app.url}
            target="_blank"
            rel="noopener noreferrer"
            className="px-2.5 py-1 rounded-md bg-slate-700/50 text-xs text-slate-300 hover:bg-slate-700 transition-colors"
          >
            Open
          </a>
        )}
        {app.studio_url && (
          <a
            href={app.studio_url}
            target="_blank"
            rel="noopener noreferrer"
            className="px-2.5 py-1 rounded-md bg-indigo-500/15 text-xs text-indigo-400 hover:bg-indigo-500/25 transition-colors"
          >
            Studio
          </a>
        )}
        <button className="px-2.5 py-1 rounded-md bg-slate-700/50 text-xs text-slate-300 hover:bg-slate-700 transition-colors">
          Logs
        </button>
      </div>
    </div>
  )
}
