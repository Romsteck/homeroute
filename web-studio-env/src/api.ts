import type {
  Environment,
  PipelineRun,
  AppDocs,
  DbTable,
  DbQueryResult,
  LogEntry,
} from "./types";

const API_BASE = "/api";

async function fetchJson<T>(url: string, init?: RequestInit, fallback?: T): Promise<T> {
  try {
    const res = await fetch(url, init);
    if (!res.ok) {
      if (fallback !== undefined) return fallback;
      throw new Error(`API error: ${res.status} ${res.statusText}`);
    }
    const json = await res.json();

    // Unwrap MCP format: { data: { content: [{ text: "..." }] } }
    if (json.data?.content?.[0]?.text) {
      try {
        const parsed = JSON.parse(json.data.content[0].text);
        return parsed as T;
      } catch { /* not MCP text, continue */ }
    }
    // Unwrap { data: <actual value> } but only if data is not an MCP envelope
    if (json.data !== undefined && !json.data?.content) return json.data as T;
    // Unwrap { success, environments/pipelines/... }
    if (json.success !== undefined) {
      const keys = Object.keys(json).filter(k => k !== 'success');
      if (keys.length === 1) return json[keys[0]] as T;
    }
    return json as T;
  } catch (e) {
    if (fallback !== undefined) return fallback;
    throw e;
  }
}

// --- Environment ---

/** Transform API environment data to our frontend types. */
function mapEnv(raw: Record<string, unknown>): Environment {
  const envTypeMap: Record<string, Environment["type"]> = {
    development: "dev", dev: "dev",
    acceptance: "acc", acc: "acc",
    production: "prod", prod: "prod",
  };
  const rawApps = (raw.apps as Array<Record<string, unknown>>) || [];
  return {
    slug: raw.slug as string,
    name: raw.name as string,
    type: envTypeMap[String(raw.env_type || raw.type || "dev")] || "dev",
    ipv4_address: raw.ipv4_address as string | undefined,
    apps: rawApps.map((a) => ({
      slug: a.slug as string,
      name: a.name as string,
      container_ip: (raw.ipv4_address || "") as string,
      status: a.running ? "running" as const : "stopped" as const,
      stack: (a.stack as string) || "unknown",
      port: (a.port as number) || 3000,
    })),
  };
}

export async function getEnvironment(envSlug: string): Promise<Environment> {
  try {
    // Try single env endpoint
    const res = await fetch(`${API_BASE}/environments/${envSlug}`);
    if (res.ok) {
      const json = await res.json();
      const raw = json.data || json.environment || json;
      if (raw.slug) return mapEnv(raw);
    }
    // Fallback: list all and filter
    const listRes = await fetch(`${API_BASE}/environments`);
    if (!listRes.ok) throw new Error(`HTTP ${listRes.status}`);
    const listJson = await listRes.json();
    const envs = listJson.environments || listJson.data || [];
    const raw = envs.find((e: Record<string, unknown>) => e.slug === envSlug);
    if (!raw) throw new Error(`Environment ${envSlug} not found`);
    return mapEnv(raw);
  } catch (e) {
    throw new Error(`Failed to load environment: ${(e as Error).message}`);
  }
}

// --- Logs ---

export async function getAppLogs(
  envSlug: string,
  appSlug: string,
  lines = 100,
): Promise<LogEntry[]> {
  const raw = await fetchJson<any>(
    `${API_BASE}/environments/${envSlug}/apps/${appSlug}/logs?lines=${lines}`,
    undefined,
    null,
  );
  // API returns { logs: "raw journalctl text", slug: "..." }
  // Parse the raw text into LogEntry[]
  const text = typeof raw === "string" ? raw : raw?.logs || "";
  if (!text) return [];
  return text
    .split("\n")
    .filter(Boolean)
    .map((line: string) => {
      // journalctl format: "Mar 29 19:48:42 hostname unit[pid]: message"
      const match = line.match(/^(\w+ \d+ [\d:]+) \S+ \S+: (.*)$/);
      if (match) {
        const message = match[2];
        const level =
          /error|panic|fatal/i.test(message) ? "ERROR"
          : /warn/i.test(message) ? "WARN"
          : /debug/i.test(message) ? "DEBUG"
          : "INFO";
        return { timestamp: match[1], level, message };
      }
      return { timestamp: "", level: "INFO", message: line };
    });
}

// --- Database ---

export async function getDbTables(
  envSlug: string,
  appSlug: string,
): Promise<DbTable[]> {
  return fetchJson<DbTable[]>(
    `${API_BASE}/environments/${envSlug}/db/tables?app_slug=${appSlug}`,
    undefined,
    [],
  );
}

export async function queryDb(
  envSlug: string,
  query: string,
  appSlug: string,
): Promise<DbQueryResult> {
  return fetchJson<DbQueryResult>(
    `${API_BASE}/environments/${envSlug}/db/query`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query, app_slug: appSlug }),
    },
  );
}

// --- Pipelines ---

export async function getPipelines(appSlug: string): Promise<PipelineRun[]> {
  try {
    const res = await fetch(`${API_BASE}/pipelines?app_slug=${appSlug}`);
    if (!res.ok) return [];
    const json = await res.json();
    // Handle both direct array and wrapped formats
    if (Array.isArray(json)) return json;
    if (Array.isArray(json.data)) return json.data;
    if (Array.isArray(json.pipelines)) return json.pipelines;
    // MCP format: { data: { content: [{ text: "[]" }] } }
    const text = json.data?.content?.[0]?.text;
    if (text) {
      const parsed = JSON.parse(text);
      if (Array.isArray(parsed)) return parsed;
    }
    return [];
  } catch {
    return [];
  }
}

export async function triggerPromotion(appSlug: string): Promise<void> {
  await fetch(`${API_BASE}/pipelines/promote`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ app_slug: appSlug }),
  });
}

// --- Docs ---

export async function getAppDocs(appSlug: string): Promise<AppDocs> {
  return fetchJson<AppDocs>(`${API_BASE}/docs/${appSlug}`, undefined, { app_id: appSlug, sections: [] });
}

export async function updateAppDoc(
  appSlug: string,
  section: string,
  content: string,
): Promise<void> {
  await fetch(`${API_BASE}/docs/${appSlug}/${section}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ content }),
  });
}
