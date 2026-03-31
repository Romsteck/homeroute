import { Link } from 'react-router-dom'
import { StatusBadge } from './StatusBadge'
import type { PipelineRun } from '../types'

interface PipelineRowProps {
  pipeline: PipelineRun
}

function formatTime(iso: string): string {
  const d = new Date(iso)
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' }) +
    ' ' +
    d.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' })
}

function formatDuration(startIso: string, endIso?: string): string {
  const start = new Date(startIso).getTime()
  const end = endIso ? new Date(endIso).getTime() : Date.now()
  const seconds = Math.floor((end - start) / 1000)
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  const remaining = seconds % 60
  return `${minutes}m ${remaining}s`
}

export function PipelineRow({ pipeline }: PipelineRowProps) {
  return (
    <Link
      to={`/pipelines/${pipeline.id}`}
      className="flex items-center gap-4 px-4 py-3 rounded-lg border border-white/5 hover:border-white/10 transition-colors cursor-pointer"
      style={{ background: '#1e1e3a' }}
    >
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-[#e2e8f0]">{pipeline.app_name}</span>
          <span className="text-xs text-white/30">v{pipeline.version}</span>
        </div>
        <div className="flex items-center gap-1 mt-0.5">
          <span className="text-xs text-white/40">{pipeline.source_env}</span>
          <span className="text-xs text-white/15">&rarr;</span>
          <span className="text-xs text-white/40">{pipeline.target_env}</span>
        </div>
      </div>

      <div className="flex items-center gap-1">
        {pipeline.steps.map((step) => (
          <div
            key={step.name}
            title={`${step.name}: ${step.status}`}
            className={`w-2 h-2 rounded-full ${
              step.status === 'success' ? 'bg-emerald-400' :
              step.status === 'running' ? 'bg-amber-400 animate-pulse' :
              step.status === 'failed' ? 'bg-red-400' :
              step.status === 'skipped' ? 'bg-white/10' :
              'bg-white/20'
            }`}
          />
        ))}
      </div>

      <StatusBadge status={pipeline.status} />

      <div className="text-right min-w-[100px]">
        <p className="text-xs text-white/40">{formatTime(pipeline.started_at)}</p>
        <p className="text-xs text-white/20">{formatDuration(pipeline.started_at, pipeline.finished_at)}</p>
      </div>
    </Link>
  )
}
