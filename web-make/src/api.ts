import type {
  Environment,
  EnvApp,
  AppInfo,
  AppEnvEntry,
  PipelineRun,
  PipelineConfig,
  GateApproval,
  DbTable,
  DbSchema,
  DbQueryResult,
  DbFilter,
} from './types'
import { isDevEnv } from './types'

// API is served on the same origin (homeroute Rust serves both SPA + API)
const API_BASE = '/api'

// ── Mock fallback flag ──────────────────────────────────────────
// Set to true to force mock data (useful for dev without backend).
const FORCE_MOCK = false

// ── Helpers ─────────────────────────────────────────────────────

const BASE_DOMAIN = 'mynetwk.biz'

/** Enrich an EnvApp from backend with computed frontend fields. */
function enrichApp(app: EnvApp, env?: Environment): EnvApp {
  const isDev = env ? isDevEnv(env.env_type) : false
  return {
    ...app,
    status: app.running ? 'running' : 'stopped',
    url: env?.slug ? `https://${app.slug}.${env.slug}.${BASE_DOMAIN}` : undefined,
    studio_url: isDev && env?.slug ? `https://studio.${env.slug}.${BASE_DOMAIN}/?folder=/apps/${app.slug}` : undefined,
  }
}

// ── Mock data (fallback when API unavailable) ───────────────────

const MOCK_ENVIRONMENTS: Environment[] = [
  {
    id: 'env-dev',
    slug: 'dev',
    name: 'Development',
    env_type: 'dev',
    host_id: 'local',
    container_name: 'env-dev',
    ipv4_address: '10.0.0.200',
    status: 'running',
    agent_connected: true,
    agent_version: '0.1.0',
    last_heartbeat: new Date().toISOString(),
    apps: [
      { slug: 'trader', name: 'Trader', stack: 'axum-vite', port: 3000, version: '1.2.0', running: true, has_db: true },
      { slug: 'wallet', name: 'Wallet', stack: 'axum-vite', port: 3001, version: '0.9.1', running: true, has_db: true },
    ],
    created_at: '2025-01-01T00:00:00Z',
  },
  {
    id: 'env-prod',
    slug: 'prod',
    name: 'Production',
    env_type: 'prod',
    host_id: 'medion',
    container_name: 'env-prod',
    ipv4_address: '10.0.0.254',
    status: 'running',
    agent_connected: true,
    agent_version: '0.1.0',
    last_heartbeat: new Date().toISOString(),
    apps: [
      { slug: 'trader', name: 'Trader', stack: 'axum-vite', port: 3000, version: '1.1.0', running: true, has_db: true },
      { slug: 'wallet', name: 'Wallet', stack: 'axum-vite', port: 3001, version: '0.9.0', running: false, has_db: true },
    ],
    created_at: '2025-01-01T00:00:00Z',
  },
]

const MOCK_PIPELINES: PipelineRun[] = [
  {
    id: 'pipe-001',
    app_slug: 'trader',
    app_name: 'Trader',
    source_env: 'dev',
    target_env: 'prod',
    version: '1.2.0',
    status: 'success',
    steps: [
      { name: 'Build', status: 'success', duration_ms: 12000 },
      { name: 'Test', status: 'success', duration_ms: 8000 },
      { name: 'Deploy', status: 'success', duration_ms: 5000 },
    ],
    started_at: new Date(Date.now() - 3600000).toISOString(),
    finished_at: new Date(Date.now() - 3500000).toISOString(),
    triggered_by: 'manual',
  },
]

// ── Environment API ─────────────────────────────────────────────

export async function fetchEnvironments(): Promise<Environment[]> {
  if (FORCE_MOCK) return MOCK_ENVIRONMENTS

  try {
    const res = await fetch(`${API_BASE}/environments`)
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const json = await res.json()
    // API returns { success: true, environments: [...] }
    const envs = json.environments || json.data || []
    // Ensure it's an array
    return Array.isArray(envs) ? envs : []
  } catch (e) {
    console.warn('fetchEnvironments failed, using mock data:', e)
    return MOCK_ENVIRONMENTS
  }
}

export async function fetchEnvironment(slug: string): Promise<Environment | null> {
  if (FORCE_MOCK) return MOCK_ENVIRONMENTS.find((e) => e.slug === slug) ?? null

  try {
    const res = await fetch(`${API_BASE}/environments/${slug}`)
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const json = await res.json()
    return json.data || null
  } catch (e) {
    console.warn('fetchEnvironment failed, using mock data:', e)
    return MOCK_ENVIRONMENTS.find((e2) => e2.slug === slug) ?? null
  }
}

export async function createEnvironment(data: {
  name: string
  slug: string
  env_type?: string
  host_id?: string
}): Promise<Environment> {
  const res = await fetch(`${API_BASE}/environments`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  })
  if (!res.ok) throw new Error('Failed to create environment')
  const json = await res.json()
  return json.data
}

export async function deleteEnvironment(slug: string): Promise<void> {
  const res = await fetch(`${API_BASE}/environments/${slug}`, { method: 'DELETE' })
  if (!res.ok) throw new Error('Failed to delete environment')
}

export async function startEnvironment(slug: string): Promise<void> {
  const res = await fetch(`${API_BASE}/environments/${slug}/start`, { method: 'POST' })
  if (!res.ok) throw new Error('Failed to start environment')
}

export async function stopEnvironment(slug: string): Promise<void> {
  const res = await fetch(`${API_BASE}/environments/${slug}/stop`, { method: 'POST' })
  if (!res.ok) throw new Error('Failed to stop environment')
}

// ── Environment apps ────────────────────────────────────────────

export async function fetchApps(envSlug: string): Promise<EnvApp[]> {
  // Apps are embedded in the environment object — extract them from there
  const envs = await fetchEnvironments()
  const env = envs.find((e) => e.slug === envSlug)
  return (env?.apps || []).map((a) => enrichApp(a, env))
}

export async function fetchAppDetail(appSlug: string): Promise<AppInfo> {
  const envs = await fetchEnvironments()
  const environments: AppEnvEntry[] = []

  for (const env of envs) {
    // Use apps from the environment object directly if available
    let apps: EnvApp[]
    if (env.apps && env.apps.length > 0) {
      apps = env.apps.map((a) => enrichApp(a, env))
    } else {
      apps = await fetchApps(env.slug)
    }
    const app = apps.find((a) => a.slug === appSlug)
    if (app) {
      environments.push({
        env_slug: env.slug,
        env_name: env.name,
        env_type: env.env_type,
        version: app.version || 'unknown',
        status: app.running ? 'running' : 'stopped',
        last_deploy: '',
      })
    }
  }

  // Find the first match to get name/stack
  const firstEnv = envs.find((e) =>
    (e.apps || []).some((a) => a.slug === appSlug),
  )
  const firstApp = firstEnv?.apps?.find((a) => a.slug === appSlug)

  return {
    slug: appSlug,
    name: firstApp?.name || appSlug,
    stack: firstApp?.stack || 'unknown',
    environments,
  }
}

// ── Pipeline API ────────────────────────────────────────────────

export async function fetchPipelines(limit?: number): Promise<PipelineRun[]> {
  if (FORCE_MOCK) return MOCK_PIPELINES

  try {
    const res = await fetch(`${API_BASE}/pipelines${limit ? `?limit=${limit}` : ''}`)
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const json = await res.json()
    const data = json.data
    // MCP returns nested content
    if (data?.content) {
      const textContent = data.content.find((c: any) => c.type === 'text')
      if (textContent) return JSON.parse(textContent.text).runs || []
    }
    return data?.runs || data || []
  } catch (e) {
    console.warn('fetchPipelines failed, using mock data:', e)
    return MOCK_PIPELINES
  }
}

export async function triggerPipeline(
  appSlug: string,
  sourceEnv: string,
  targetEnv: string,
): Promise<{ id: string }> {
  const res = await fetch(`${API_BASE}/pipelines/promote`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      app_slug: appSlug,
      version: 'latest',
      source_env: sourceEnv,
      target_env: targetEnv,
    }),
  })
  if (!res.ok) throw new Error('Failed to trigger pipeline')
  const json = await res.json()
  return json.data || { id: 'unknown' }
}

export async function fetchPipeline(id: string): Promise<PipelineRun> {
  const res = await fetch(`${API_BASE}/pipelines/${id}`)
  if (!res.ok) throw new Error(`Failed to fetch pipeline: ${res.status}`)
  const data = await res.json()
  return data.data || data
}

export async function fetchPipelineConfig(appSlug: string): Promise<PipelineConfig | null> {
  try {
    const res = await fetch(`${API_BASE}/pipelines/config/${appSlug}`)
    if (!res.ok) return null
    const data = await res.json()
    return data.data || data
  } catch {
    return null
  }
}

export async function savePipelineConfig(config: PipelineConfig): Promise<void> {
  const res = await fetch(`${API_BASE}/pipelines/config`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config),
  })
  if (!res.ok) throw new Error(`Failed to save config: ${res.status}`)
}

export async function approveGate(gateId: string): Promise<void> {
  const res = await fetch(`${API_BASE}/pipelines/gates/${gateId}/approve`, { method: 'POST' })
  if (!res.ok) throw new Error(`Failed to approve gate: ${res.status}`)
}

export async function rejectGate(gateId: string): Promise<void> {
  const res = await fetch(`${API_BASE}/pipelines/gates/${gateId}/reject`, { method: 'POST' })
  if (!res.ok) throw new Error(`Failed to reject gate: ${res.status}`)
}

export async function fetchPendingGates(): Promise<GateApproval[]> {
  try {
    const res = await fetch(`${API_BASE}/pipelines/gates/pending`)
    if (!res.ok) return []
    const data = await res.json()
    return data.data || data || []
  } catch {
    return []
  }
}

// ── Environment monitoring & control ────────────────────────────

export async function fetchEnvMonitoring(envSlug: string): Promise<any> {
  try {
    const res = await fetch(`${API_BASE}/environments/${envSlug}/monitoring`)
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const json = await res.json()
    return json.data || {}
  } catch {
    return {}
  }
}

export async function controlApp(
  envSlug: string,
  appSlug: string,
  action: string,
): Promise<any> {
  const res = await fetch(`${API_BASE}/environments/${envSlug}/apps/${appSlug}/control`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ action }),
  })
  if (!res.ok) throw new Error('Failed to control app')
  const json = await res.json()
  return json.data || {}
}

export async function fetchAppLogs(
  envSlug: string,
  appSlug: string,
  lines?: number,
): Promise<string> {
  try {
    const params = lines ? `?lines=${lines}` : ''
    const res = await fetch(`${API_BASE}/environments/${envSlug}/apps/${appSlug}/logs${params}`)
    if (!res.ok) return 'Failed to fetch logs'
    const json = await res.json()
    return json.data?.logs || json.data || ''
  } catch {
    return 'Failed to fetch logs'
  }
}

// ── DB Explorer ─────────────────────────────────────────────────

export async function fetchDbTables(envSlug: string, appSlug?: string): Promise<DbTable[]> {
  const params = appSlug ? `?app_slug=${appSlug}` : ''
  const res = await fetch(`${API_BASE}/environments/${envSlug}/db/tables${params}`)
  if (!res.ok) throw new Error('Failed to fetch tables')
  const json = await res.json()
  return json.data?.tables || json.tables || []
}

export async function fetchDbSchema(envSlug: string, table: string, appSlug?: string): Promise<DbSchema> {
  const params = new URLSearchParams({ table })
  if (appSlug) params.set('app_slug', appSlug)
  const res = await fetch(`${API_BASE}/environments/${envSlug}/db/schema?${params}`)
  if (!res.ok) throw new Error('Failed to fetch schema')
  const json = await res.json()
  return json.data || json
}

export async function queryDbData(
  envSlug: string,
  table: string,
  options: {
    limit?: number
    offset?: number
    order_by?: string
    order_desc?: boolean
    filters?: DbFilter[]
    app_slug?: string
  } = {},
): Promise<DbQueryResult> {
  const params = new URLSearchParams({ table })
  if (options.limit != null) params.set('limit', String(options.limit))
  if (options.offset != null) params.set('offset', String(options.offset))
  if (options.order_by) params.set('order_by', options.order_by)
  if (options.order_desc != null) params.set('order_desc', String(options.order_desc))
  if (options.app_slug) params.set('app_slug', options.app_slug)
  if (options.filters && options.filters.length > 0) {
    params.set('filters', JSON.stringify(options.filters))
  }
  const res = await fetch(`${API_BASE}/environments/${envSlug}/db/query?${params}`)
  if (!res.ok) throw new Error('Failed to query data')
  const json = await res.json()
  return json.data || json
}

export async function countDbRows(
  envSlug: string,
  table: string,
  appSlug?: string,
  filters?: DbFilter[],
): Promise<number> {
  const params = new URLSearchParams({ table })
  if (appSlug) params.set('app_slug', appSlug)
  if (filters && filters.length > 0) params.set('filters', JSON.stringify(filters))
  const res = await fetch(`${API_BASE}/environments/${envSlug}/db/count?${params}`)
  if (!res.ok) throw new Error('Failed to count rows')
  const json = await res.json()
  return json.data?.count ?? json.count ?? 0
}

export async function insertDbRows(
  envSlug: string,
  appSlug: string,
  table: string,
  rows: Record<string, any>[],
): Promise<void> {
  const res = await fetch(`${API_BASE}/environments/${envSlug}/db/rows`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ app_slug: appSlug, table, rows }),
  })
  if (!res.ok) {
    const json = await res.json().catch(() => ({}))
    throw new Error(json.error || 'Failed to insert rows')
  }
}

export async function updateDbRows(
  envSlug: string,
  appSlug: string,
  table: string,
  updates: Record<string, any>,
  filters: DbFilter[],
): Promise<void> {
  const res = await fetch(`${API_BASE}/environments/${envSlug}/db/rows`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ app_slug: appSlug, table, updates, filters }),
  })
  if (!res.ok) {
    const json = await res.json().catch(() => ({}))
    throw new Error(json.error || 'Failed to update rows')
  }
}

export async function deleteDbRows(
  envSlug: string,
  appSlug: string,
  table: string,
  filters: DbFilter[],
): Promise<void> {
  const res = await fetch(`${API_BASE}/environments/${envSlug}/db/rows`, {
    method: 'DELETE',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ app_slug: appSlug, table, filters }),
  })
  if (!res.ok) {
    const json = await res.json().catch(() => ({}))
    throw new Error(json.error || 'Failed to delete rows')
  }
}

// ── Cross-env monitoring ────────────────────────────────────────

export async function fetchMonitoringSummary(): Promise<Environment[]> {
  try {
    const res = await fetch(`${API_BASE}/monitoring/envs`)
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const json = await res.json()
    return json.environments || json.data || []
  } catch (e) {
    console.warn('fetchMonitoringSummary failed, using mock data:', e)
    return MOCK_ENVIRONMENTS
  }
}
