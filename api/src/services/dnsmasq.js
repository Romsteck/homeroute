import { readFile } from 'fs/promises';
import { existsSync } from 'fs';

function getConfigPath() {
  return process.env.DNS_DHCP_CONFIG_PATH || '/var/lib/server-dashboard/dns-dhcp-config.json';
}

function getLeasesPath() {
  return process.env.DNSMASQ_LEASES || '/var/lib/server-dashboard/dhcp-leases';
}

export async function getDnsConfig() {
  try {
    const configPath = getConfigPath();
    if (!existsSync(configPath)) {
      return { success: false, error: 'Config file not found' };
    }

    const content = await readFile(configPath, 'utf-8');
    const json = JSON.parse(content);

    // Translate JSON config to frontend-compatible format
    const config = {
      interface: json.dhcp?.interface || null,
      dhcpRange: json.dhcp?.enabled
        ? `${json.dhcp.range_start},${json.dhcp.range_end},${json.dhcp.netmask},${json.dhcp.default_lease_time_secs}`
        : null,
      dhcpOptions: [],
      dnsServers: json.dns?.upstream_servers || [],
      domain: json.dns?.local_domain || null,
      cacheSize: json.dns?.cache_size || null,
      ipv6: {
        raEnabled: json.ipv6?.ra_enabled || false,
        dhcpRange: json.ipv6?.enabled ? json.ipv6.ra_prefix : null,
      },
      wildcardAddress: json.dns?.wildcard_ipv4
        ? { domain: json.dns.local_domain, ip: json.dns.wildcard_ipv4 }
        : null,
      comments: []
    };

    // Build dhcpOptions from config
    if (json.dhcp?.gateway) {
      config.dhcpOptions.push(`3,${json.dhcp.gateway}`);
    }
    if (json.dhcp?.dns_server) {
      config.dhcpOptions.push(`6,${json.dhcp.dns_server}`);
    }

    return { success: true, config, raw: content };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function getDhcpLeases() {
  try {
    const leasesPath = getLeasesPath();
    if (!existsSync(leasesPath)) {
      return { success: true, leases: [] };
    }

    const content = await readFile(leasesPath, 'utf-8');
    const leases = parseLeases(content);
    return { success: true, leases };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

function parseLeases(content) {
  const lines = content.split('\n').filter(l => l.trim());
  const leases = [];

  for (const line of lines) {
    try {
      const parts = line.split(' ');

      // Validate minimum required fields (timestamp, MAC, IP)
      if (parts.length < 3) {
        console.warn(`[DHCP] Skipping invalid lease line (not enough parts): ${line}`);
        continue;
      }

      leases.push({
        expiration: new Date(parseInt(parts[0]) * 1000).toISOString(),
        expirationTimestamp: parseInt(parts[0]),
        mac: parts[1],
        ip: parts[2],
        hostname: parts[3] && parts[3] !== '*' ? parts[3] : null,
        clientId: parts[4] || null
      });
    } catch (error) {
      console.warn(`[DHCP] Skipping invalid lease line: ${line}`, error.message);
    }
  }

  return leases.sort((a, b) => {
    // Sort by IP address
    const ipA = a.ip.split('.').map(Number);
    const ipB = b.ip.split('.').map(Number);
    for (let i = 0; i < 4; i++) {
      if (ipA[i] !== ipB[i]) return ipA[i] - ipB[i];
    }
    return 0;
  });
}
