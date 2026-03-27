// --- Environments ---

export type EnvType = 'dev' | 'acc' | 'prod'

export type EnvStatus = 'running' | 'stopped' | 'pending' | 'provisioning' | 'disconnected' | 'error'

export interface Environment {
  id: string
  slug: string
  name: string
  env_type: EnvType
  host_id: string
  container_name: string
  ipv4_address: string | null
  status: EnvStatus
  agent_connected: boolean
  agent_version: string | null
  last_heartbeat: string | null
  apps: EnvApp[]
  created_at: string
}

export interface EnvApp {
  slug: string
  name: string
  stack: string
  port: number
  version: string | null
  running: boolean
  has_db: boolean
  // Computed by frontend for compatibility
  status?: AppStatus
  url?: string
  studio_url?: string
}

// --- Apps ---

export type AppStatus = 'running' | 'stopped' | 'error' | 'deploying'

export interface AppInfo {
  slug: string
  name: string
  stack: string
  environments: AppEnvEntry[]
}

export interface AppEnvEntry {
  env_slug: string
  env_name: string
  env_type: EnvType
  version: string
  status: AppStatus
  last_deploy?: string
}

// --- Pipelines ---

export type PipelineStatus = 'pending' | 'running' | 'success' | 'failed' | 'cancelled'

export type StepStatus = 'pending' | 'running' | 'success' | 'failed' | 'skipped'

export interface PipelineStep {
  name: string
  status: StepStatus
  duration_ms?: number
  log?: string
}

export interface PipelineRun {
  id: string
  app_slug: string
  app_name: string
  source_env: string
  target_env: string
  version: string
  status: PipelineStatus
  steps: PipelineStep[]
  started_at: string
  finished_at?: string
  triggered_by: string
}

// --- DB Explorer ---

export interface DbTable {
  name: string
  row_count: number
  column_count: number
}

export interface DbColumn {
  name: string
  data_type: string
  nullable: boolean
  primary_key: boolean
  default_value?: string
}

export interface DbSchema {
  table_name: string
  columns: DbColumn[]
  relations: DbRelation[]
}

export interface DbRelation {
  from_table: string
  from_column: string
  to_table: string
  to_column: string
  relation_type: string
}

export interface DbQueryResult {
  columns: string[]
  rows: Record<string, string | number | boolean | null>[]
  total_count: number
}
