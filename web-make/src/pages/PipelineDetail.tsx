import { useEffect, useState, useCallback } from 'react'
import { useParams, Link } from 'react-router-dom'
import { fetchPipeline, approveGate, rejectGate } from '../api'
import { StatusBadge } from '../components/StatusBadge'
import type { PipelineRun } from '../types'

function StepIcon({ status }: { status: string }) {
  switch (status) {
    case 'success':
      return (
        <div className="w-6 h-6 rounded-full bg-emerald-500/20 border border-emerald-500/50 flex items-center justify-center">
          <span className="text-emerald-400 text-xs">&#10003;</span>
        </div>
      )
    case 'running':
      return <div className="w-6 h-6 rounded-full border-2 border-[#7c3aed]/50 border-t-[#7c3aed] animate-spin" />
    case 'failed':
      return (
        <div className="w-6 h-6 rounded-full bg-red-500/20 border border-red-500/50 flex items-center justify-center">
          <span className="text-red-400 text-xs">&#10007;</span>
        </div>
      )
    case 'skipped':
      return (
        <div className="w-6 h-6 rounded-full bg-white/5 border border-white/10 flex items-center justify-center">
          <span className="text-white/20 text-xs">-</span>
        </div>
      )
    default:
      return <div className="w-6 h-6 rounded-full bg-white/5 border border-white/10" />
  }
}

function formatDuration(startedAt?: string, finishedAt?: string): string {
  if (!startedAt) return ''
  const start = new Date(startedAt).getTime()
  const end = finishedAt ? new Date(finishedAt).getTime() : Date.now()
  const ms = end - start
  if (ms < 1000) return `${ms}ms`
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`
  return `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`
}

export function PipelineDetail() {
  const { id } = useParams<{ id: string }>()
  const [pipeline, setPipeline] = useState<PipelineRun | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [expandedStep, setExpandedStep] = useState<string | null>(null)
  const [gateLoading, setGateLoading] = useState(false)

  const load = useCallback(() => {
    if (!id) return
    fetchPipeline(id)
      .then(setPipeline)
      .catch((e) => setError(e.message))
  }, [id])

  useEffect(() => {
    load()
  }, [load])

  // Poll every 3s while running/pending/waiting_gate
  useEffect(() => {
    if (!pipeline) return
    const isActive = pipeline.status === 'running' || pipeline.status === 'pending' || pipeline.status === 'waiting_gate'
    if (!isActive) return

    const interval = setInterval(load, 3000)
    return () => clearInterval(interval)
  }, [pipeline?.status, load])

  const handleApproveGate = async () => {
    if (!pipeline) return
    setGateLoading(true)
    try {
      // The gate ID follows the convention: the pipeline id itself or a dedicated gate id
      // For now we pass the pipeline id; the backend resolves the pending gate
      await approveGate(pipeline.id)
      load()
    } catch (e) {
      alert('Failed to approve: ' + (e as Error).message)
    } finally {
      setGateLoading(false)
    }
  }

  const handleRejectGate = async () => {
    if (!pipeline) return
    setGateLoading(true)
    try {
      await rejectGate(pipeline.id)
      load()
    } catch (e) {
      alert('Failed to reject: ' + (e as Error).message)
    } finally {
      setGateLoading(false)
    }
  }

  if (error) {
    return (
      <div className="p-8 text-center">
        <p className="text-red-400">{error}</p>
        <Link to="/pipelines" className="text-[#7c3aed] hover:underline mt-4 inline-block">
          Back to pipelines
        </Link>
      </div>
    )
  }

  if (!pipeline) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="w-6 h-6 border-2 border-[#7c3aed]/30 border-t-[#7c3aed] rounded-full animate-spin" />
      </div>
    )
  }

  const totalDuration = formatDuration(pipeline.started_at, pipeline.finished_at)

  return (
    <div className="space-y-6 max-w-3xl mx-auto">
      {/* Breadcrumb */}
      <div className="flex items-center gap-2">
        <Link to="/pipelines" className="text-white/30 hover:text-white/60 text-sm">
          &larr; Pipelines
        </Link>
      </div>

      {/* Header card */}
      <div className="rounded-xl border border-white/5 p-6" style={{ background: '#1e1e3a' }}>
        <div className="flex items-start justify-between">
          <div>
            <h1 className="text-xl font-bold text-[#e2e8f0]">{pipeline.app_slug}</h1>
            <p className="text-sm text-white/40 mt-1">
              v{pipeline.version} &middot; {pipeline.source_env} &rarr; {pipeline.target_env}
            </p>
            {pipeline.triggered_by && (
              <p className="text-xs text-white/20 mt-1">Triggered by {pipeline.triggered_by}</p>
            )}
          </div>
          <div className="text-right">
            <StatusBadge status={pipeline.status} />
            {totalDuration && <p className="text-xs text-white/30 mt-2">{totalDuration}</p>}
          </div>
        </div>
      </div>

      {/* Steps Timeline */}
      <div className="rounded-xl border border-white/5 p-6" style={{ background: '#1e1e3a' }}>
        <h2 className="text-sm font-medium text-white/50 mb-4">Pipeline Steps</h2>
        <div className="space-y-1">
          {pipeline.steps.map((step, i) => (
            <div key={step.name}>
              <button
                onClick={() => setExpandedStep(expandedStep === step.name ? null : step.name)}
                className="w-full flex items-center gap-3 p-3 rounded-lg hover:bg-white/5 transition-colors text-left"
              >
                {/* Icon + connector */}
                <div className="flex flex-col items-center">
                  <StepIcon status={step.status} />
                  {i < pipeline.steps.length - 1 && (
                    <div
                      className={`w-px h-4 mt-1 ${
                        step.status === 'success' ? 'bg-emerald-500/30' : 'bg-white/10'
                      }`}
                    />
                  )}
                </div>

                <div className="flex-1 min-w-0">
                  <div className="flex items-center justify-between">
                    <span
                      className={`text-sm font-medium ${
                        step.status === 'running'
                          ? 'text-[#7c3aed]'
                          : step.status === 'success'
                            ? 'text-white/80'
                            : step.status === 'failed'
                              ? 'text-red-400'
                              : 'text-white/30'
                      }`}
                    >
                      {step.name}
                    </span>
                    <span className="text-xs text-white/20">
                      {formatDuration(step.started_at, step.finished_at)}
                    </span>
                  </div>
                </div>
              </button>

              {/* Expanded log output */}
              {expandedStep === step.name && (step.output || step.log) && (
                <div className="ml-12 mb-2 p-3 rounded-lg bg-black/30 border border-white/5">
                  <pre className="text-xs text-white/50 whitespace-pre-wrap font-mono max-h-64 overflow-y-auto">
                    {step.output || step.log}
                  </pre>
                </div>
              )}
            </div>
          ))}
        </div>
      </div>

      {/* Gate approval buttons */}
      {pipeline.status === 'waiting_gate' && (
        <div className="rounded-xl border border-amber-500/20 p-6" style={{ background: '#1e1e3a' }}>
          <h2 className="text-sm font-medium text-amber-400 mb-3">Approval Required</h2>
          <p className="text-sm text-white/50 mb-4">
            This pipeline is waiting for approval to continue.
          </p>
          <div className="flex gap-3">
            <button
              onClick={handleApproveGate}
              disabled={gateLoading}
              className="px-4 py-2 bg-emerald-600 hover:bg-emerald-700 disabled:opacity-50 text-white text-sm font-medium rounded-lg transition-colors"
            >
              {gateLoading ? 'Processing...' : 'Approve'}
            </button>
            <button
              onClick={handleRejectGate}
              disabled={gateLoading}
              className="px-4 py-2 bg-red-600/20 hover:bg-red-600/30 disabled:opacity-50 text-red-400 text-sm font-medium rounded-lg transition-colors border border-red-500/20"
            >
              Reject
            </button>
          </div>
        </div>
      )}

      {/* Cancel button for running pipelines */}
      {(pipeline.status === 'running' || pipeline.status === 'pending') && (
        <div className="flex justify-end">
          <button className="px-4 py-2 text-sm text-white/30 hover:text-red-400 hover:bg-red-500/10 rounded-lg transition-colors border border-white/5">
            Cancel Pipeline
          </button>
        </div>
      )}
    </div>
  )
}
