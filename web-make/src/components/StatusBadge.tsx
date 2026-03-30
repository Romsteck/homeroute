import type { AppStatus, PipelineStatus, EnvType } from '../types'

const statusColors: Record<string, string> = {
  running: 'bg-emerald-500/20 text-emerald-400',
  stopped: 'bg-white/5 text-white/40',
  error: 'bg-red-500/20 text-red-400',
  deploying: 'bg-amber-500/20 text-amber-400',
  success: 'bg-emerald-500/20 text-emerald-400',
  failed: 'bg-red-500/20 text-red-400',
  pending: 'bg-white/5 text-white/40',
  cancelled: 'bg-white/5 text-white/40',
  degraded: 'bg-amber-500/20 text-amber-400',
  provisioning: 'bg-blue-500/20 text-blue-400',
  disconnected: 'bg-orange-500/20 text-orange-400',
}

const statusDots: Record<string, string> = {
  running: 'bg-emerald-400',
  stopped: 'bg-white/30',
  error: 'bg-red-400',
  deploying: 'bg-amber-400 animate-pulse',
  success: 'bg-emerald-400',
  failed: 'bg-red-400',
  pending: 'bg-white/30',
  cancelled: 'bg-white/30',
  degraded: 'bg-amber-400',
  provisioning: 'bg-blue-400 animate-pulse',
  disconnected: 'bg-orange-400',
}

const envTypeColors: Record<string, string> = {
  dev: 'bg-blue-500/20 text-blue-400',
  development: 'bg-blue-500/20 text-blue-400',
  acc: 'bg-amber-500/20 text-amber-400',
  acceptance: 'bg-amber-500/20 text-amber-400',
  prod: 'bg-emerald-500/20 text-emerald-400',
  production: 'bg-emerald-500/20 text-emerald-400',
}

export function StatusBadge({ status }: { status: AppStatus | PipelineStatus | string }) {
  return (
    <span className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium ${statusColors[status] ?? 'bg-white/5 text-white/40'}`}>
      <span className={`w-1.5 h-1.5 rounded-full ${statusDots[status] ?? 'bg-white/30'}`} />
      {status}
    </span>
  )
}

const envTypeLabels: Record<string, string> = {
  dev: 'DEV', development: 'DEV',
  acc: 'ACC', acceptance: 'ACC',
  prod: 'PROD', production: 'PROD',
}

export function EnvTypeBadge({ envType }: { envType: EnvType }) {
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-[10px] font-bold uppercase tracking-wider ${envTypeColors[envType] ?? 'bg-white/5 text-white/40'}`}>
      {envTypeLabels[envType] ?? envType}
    </span>
  )
}

const stackColors: Record<string, string> = {
  'next-js': 'bg-white/10 text-white/70',
  'axum-vite': 'bg-orange-500/15 text-orange-400',
  'axum': 'bg-amber-500/15 text-amber-400',
}

const stackLabels: Record<string, string> = {
  'next-js': 'Next.js',
  'axum-vite': 'Axum + Vite',
  'axum': 'Axum',
}

export function StackBadge({ stack }: { stack: string }) {
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium ${stackColors[stack] ?? 'bg-[#7c3aed]/15 text-[#a78bfa]'}`}>
      {stackLabels[stack] ?? stack}
    </span>
  )
}
