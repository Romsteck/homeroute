import { exec } from 'child_process';
import { promisify } from 'util';

const execAsync = promisify(exec);

export async function getInterfaces() {
  try {
    const { stdout } = await execAsync('ip -j addr show');
    const interfaces = JSON.parse(stdout);

    // Filter and format interfaces
    const formatted = interfaces
      .filter(iface => !iface.ifname.startsWith('veth')) // Exclude veth pairs
      .map(iface => ({
        name: iface.ifname,
        state: iface.operstate,
        mac: iface.address,
        mtu: iface.mtu,
        flags: iface.flags,
        addresses: (iface.addr_info || []).map(addr => ({
          family: addr.family,
          address: addr.local,
          prefixlen: addr.prefixlen,
          scope: addr.scope,
          label: addr.label
        }))
      }));

    return { success: true, interfaces: formatted };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function getRoutes() {
  try {
    const { stdout } = await execAsync('ip -j route show');
    const routes = JSON.parse(stdout);

    const formatted = routes.map(route => ({
      destination: route.dst,
      gateway: route.gateway || null,
      device: route.dev,
      protocol: route.protocol,
      scope: route.scope,
      source: route.prefsrc || null,
      metric: route.metric
    }));

    return { success: true, routes: formatted };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function getIpv6Routes() {
  try {
    const { stdout } = await execAsync('ip -j -6 route show');
    const routes = JSON.parse(stdout);

    const formatted = routes.map(route => ({
      destination: route.dst,
      gateway: route.gateway || null,
      device: route.dev,
      protocol: route.protocol,
      metric: route.metric
    }));

    return { success: true, routes: formatted };
  } catch (error) {
    return { success: false, error: error.message };
  }
}
