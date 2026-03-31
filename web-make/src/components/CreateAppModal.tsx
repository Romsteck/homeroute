import { useState } from 'react'
import { APP_STACKS } from '../types'
import type { AppStackType } from '../types'
import { createApp } from '../api'

interface CreateAppModalProps {
  envSlug: string
  onCreated: () => void
  onClose: () => void
}

function slugify(name: string): string {
  return name
    .toLowerCase()
    .replace(/\s+/g, '-')
    .replace(/[^a-z0-9-]/g, '')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '')
}

export function CreateAppModal({ envSlug, onCreated, onClose }: CreateAppModalProps) {
  const [name, setName] = useState('')
  const [slug, setSlug] = useState('')
  const [slugManual, setSlugManual] = useState(false)
  const [stack, setStack] = useState<AppStackType>('axum-vite')
  const [hasDb, setHasDb] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  function handleNameChange(val: string) {
    setName(val)
    if (!slugManual) {
      setSlug(slugify(val))
    }
  }

  function handleSlugChange(val: string) {
    setSlugManual(true)
    setSlug(slugify(val))
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!name.trim() || !slug.trim()) return

    setSubmitting(true)
    setError(null)
    try {
      await createApp(envSlug, { name: name.trim(), slug, stack, has_db: hasDb })
      onCreated()
      onClose()
    } catch (err: any) {
      setError(err.message || 'Failed to create app')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm" onClick={onClose}>
      <div
        className="w-full max-w-lg rounded-xl border border-white/10 shadow-2xl p-6"
        style={{ background: '#1e1e3a' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-5">
          <h2 className="text-lg font-semibold text-white/90">Create App</h2>
          <button onClick={onClose} className="text-white/30 hover:text-white/60 transition-colors">
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {error && (
          <div className="mb-4 bg-red-500/10 border border-red-500/30 rounded-lg px-3 py-2">
            <p className="text-sm text-red-400">{error}</p>
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-xs font-medium text-white/50 mb-1">Name</label>
            <input
              type="text"
              value={name}
              onChange={(e) => handleNameChange(e.target.value)}
              placeholder="My App"
              autoFocus
              className="w-full text-sm rounded-md border border-white/10 bg-white/5 text-white/80 px-3 py-2 placeholder-white/15 focus:outline-none focus:border-[#7c3aed]/50"
            />
          </div>

          <div>
            <label className="block text-xs font-medium text-white/50 mb-1">Slug</label>
            <input
              type="text"
              value={slug}
              onChange={(e) => handleSlugChange(e.target.value)}
              placeholder="my-app"
              className="w-full text-sm rounded-md border border-white/10 bg-white/5 text-white/80 font-mono px-3 py-2 placeholder-white/15 focus:outline-none focus:border-[#7c3aed]/50"
            />
          </div>

          <div>
            <label className="block text-xs font-medium text-white/50 mb-1">Stack</label>
            <select
              value={stack}
              onChange={(e) => setStack(e.target.value as AppStackType)}
              className="w-full text-sm rounded-md border border-white/10 bg-white/5 text-white/80 px-3 py-2 focus:outline-none focus:border-[#7c3aed]/50"
            >
              {APP_STACKS.map((s) => (
                <option key={s.value} value={s.value} className="bg-[#1e1e3a]">
                  {s.label}
                </option>
              ))}
            </select>
          </div>

          <div>
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={hasDb}
                onChange={(e) => setHasDb(e.target.checked)}
                className="w-4 h-4 rounded border-white/20 bg-white/5 text-[#7c3aed] focus:ring-[#7c3aed]/50"
              />
              <span className="text-sm text-white/60">Has database</span>
            </label>
          </div>

          <div className="flex justify-end gap-3 pt-3 border-t border-white/5">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 text-sm rounded-lg bg-white/5 text-white/50 hover:bg-white/10 transition-colors"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={submitting || !name.trim() || !slug.trim()}
              className="px-4 py-2 text-sm rounded-lg bg-[#7c3aed] text-white hover:bg-[#6d28d9] disabled:opacity-50 transition-colors"
            >
              {submitting ? 'Creating...' : 'Create'}
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}
