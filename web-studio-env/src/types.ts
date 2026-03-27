export interface EnvApp {
  slug: string;
  name: string;
  container_ip: string;
  status: "running" | "stopped" | "unknown";
  stack: string;
  port: number;
}

export interface Environment {
  slug: string;
  name: string;
  type: "dev" | "acc" | "prod";
  apps: EnvApp[];
  ipv4_address?: string;
  code_server_ip?: string;
  code_server_port?: number;
}

export interface Todo {
  id: string;
  title: string;
  description?: string;
  context: string;
  priority: "high" | "medium" | "low";
  status: "todo" | "in_progress" | "done";
  created_at: string;
  completed_at?: string;
}

export interface PipelineRun {
  id: string;
  app_slug: string;
  status: "pending" | "running" | "success" | "failed";
  trigger: string;
  started_at: string;
  finished_at?: string;
  steps: PipelineStep[];
}

export interface PipelineStep {
  name: string;
  status: "pending" | "running" | "success" | "failed" | "skipped";
  duration_ms?: number;
}

export interface DocSection {
  section: "meta" | "structure" | "features" | "backend" | "notes";
  content: string;
}

export interface AppDocs {
  app_id: string;
  sections: DocSection[];
}

export interface LogEntry {
  timestamp: string;
  level: string;
  message: string;
}

export interface DbTable {
  name: string;
  row_count: number;
  columns: DbColumn[];
}

export interface DbColumn {
  name: string;
  type: string;
  nullable: boolean;
  primary_key: boolean;
}

export interface DbQueryResult {
  columns: string[];
  rows: Record<string, unknown>[];
  row_count: number;
}

export type TabId = "code" | "board" | "docs" | "pipes" | "db" | "logs";
