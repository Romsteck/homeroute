// Adblock service — proxies to Rust DNS/DHCP internal API (127.0.0.1:5380)

const ADBLOCK_API = process.env.ADBLOCK_API_URL || 'http://127.0.0.1:5380';

async function fetchApi(path, options = {}) {
  const url = `${ADBLOCK_API}${path}`;
  try {
    const res = await fetch(url, {
      ...options,
      headers: { 'Content-Type': 'application/json', ...options.headers },
    });
    return await res.json();
  } catch (error) {
    return { success: false, error: `Adblock API unreachable: ${error.message}` };
  }
}

export async function getStats() {
  const data = await fetchApi('/stats');
  if (data.error) return { success: false, error: data.error };

  return {
    success: true,
    stats: {
      domainCount: data.domain_count,
      lastUpdate: data.last_update,
      sources: (data.sources || []).map(s => ({
        name: s.name,
        url: s.url,
        count: s.count
      })),
      logs: []
    }
  };
}

export async function getWhitelist() {
  const data = await fetchApi('/whitelist');
  if (data.error) return { success: false, error: data.error };
  return { success: data.success, domains: data.domains || [] };
}

export async function addToWhitelist(domain) {
  const data = await fetchApi('/whitelist', {
    method: 'POST',
    body: JSON.stringify({ domain }),
  });
  return { success: data.success, message: data.message, error: data.error };
}

export async function removeFromWhitelist(domain) {
  const data = await fetchApi(`/whitelist/${encodeURIComponent(domain)}`, {
    method: 'DELETE',
  });
  return { success: data.success, message: data.message, error: data.error };
}

export async function updateLists() {
  const data = await fetchApi('/update', { method: 'POST' });
  return {
    success: data.success,
    message: data.message || 'Update completed',
    domainCount: data.domain_count
  };
}

export async function searchBlocked(query) {
  const data = await fetchApi(`/search?q=${encodeURIComponent(query)}`);
  if (data.error) return { success: false, error: data.error };
  return { success: data.success, results: data.results || [] };
}
