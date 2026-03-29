import { useState, useEffect, useCallback } from 'react';

const MCP_URL = 'http://localhost:4010/mcp';

async function mcpCall(tool, args = {}) {
  const res = await fetch(MCP_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: Date.now(),
      method: 'tools/call',
      params: { name: tool, arguments: args }
    })
  });
  const json = await res.json();
  const text = json?.result?.content?.[0]?.text;
  return text ? JSON.parse(text) : null;
}

export default function useAppStatus(pollInterval = 10000) {
  const [apps, setApps] = useState([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    try {
      const data = await mcpCall('app.list');
      if (Array.isArray(data)) setApps(data);
    } catch (e) { console.warn('Failed to fetch app status:', e); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => {
    refresh();
    const timer = setInterval(refresh, pollInterval);
    return () => clearInterval(timer);
  }, [refresh, pollInterval]);

  const controlApp = useCallback(async (slug, action) => {
    try {
      await mcpCall(`app.${action}`, { slug });
      setTimeout(refresh, 1500);
    } catch (e) { console.warn(`Failed to ${action} ${slug}:`, e); }
  }, [refresh]);

  const startApp = useCallback((slug) => controlApp(slug, 'start'), [controlApp]);
  const stopApp = useCallback((slug) => controlApp(slug, 'stop'), [controlApp]);
  const restartApp = useCallback((slug) => controlApp(slug, 'restart'), [controlApp]);

  const startAll = useCallback(async () => {
    for (const app of apps) {
      if (app.status !== 'running') await mcpCall('app.start', { slug: app.slug });
    }
    setTimeout(refresh, 2000);
  }, [apps, refresh]);

  const stopAll = useCallback(async () => {
    for (const app of apps) {
      if (app.status === 'running') await mcpCall('app.stop', { slug: app.slug });
    }
    setTimeout(refresh, 2000);
  }, [apps, refresh]);

  return { apps, loading, refresh, startApp, stopApp, restartApp, startAll, stopAll };
}
