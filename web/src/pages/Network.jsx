import { useState, useEffect } from 'react';
import { Network as NetworkIcon, Route, ArrowRight } from 'lucide-react';
import Card from '../components/Card';
import StatusBadge from '../components/StatusBadge';
import { getInterfaces, getRoutes, getMasqueradeRules, getPortForwards } from '../api/client';

function Network() {
  const [interfaces, setInterfaces] = useState([]);
  const [routes, setRoutes] = useState([]);
  const [masquerade, setMasquerade] = useState([]);
  const [forwards, setForwards] = useState([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    async function fetchData() {
      try {
        const [ifRes, routeRes, masqRes, fwdRes] = await Promise.all([
          getInterfaces(),
          getRoutes(),
          getMasqueradeRules(),
          getPortForwards()
        ]);

        if (ifRes.data.success) setInterfaces(ifRes.data.interfaces);
        if (routeRes.data.success) setRoutes(routeRes.data.routes);
        if (masqRes.data.success) setMasquerade(masqRes.data.rules);
        if (fwdRes.data.success) setForwards(fwdRes.data.rules);
      } catch (error) {
        console.error('Error:', error);
      } finally {
        setLoading(false);
      }
    }

    fetchData();
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-400"></div>
      </div>
    );
  }

  // Separate physical from virtual interfaces
  const physicalIfaces = interfaces.filter(i =>
    i.name.startsWith('en') || i.name.startsWith('eth')
  );
  const bridgeIfaces = interfaces.filter(i =>
    i.name.startsWith('br-') || i.name.startsWith('virbr') || i.name.startsWith('lxc') || i.name === 'docker0'
  );

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Réseau</h1>

      {/* Physical Interfaces */}
      <Card title="Interfaces Physiques" icon={NetworkIcon}>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {physicalIfaces.map(iface => (
            <div key={iface.name} className="bg-gray-900 rounded-lg p-4">
              <div className="flex items-center justify-between mb-3">
                <span className="font-mono font-bold">{iface.name}</span>
                <StatusBadge status={iface.state === 'UP' ? 'up' : 'down'}>
                  {iface.state}
                </StatusBadge>
              </div>
              <div className="space-y-2 text-sm">
                <div className="flex justify-between text-gray-400">
                  <span>MAC</span>
                  <span className="font-mono text-xs">{iface.mac}</span>
                </div>
                <div className="flex justify-between text-gray-400">
                  <span>MTU</span>
                  <span className="font-mono">{iface.mtu}</span>
                </div>
                {iface.addresses?.filter(a => a.family === 'inet').map((addr, i) => (
                  <div key={i} className="flex justify-between">
                    <span className="text-gray-400">IPv4</span>
                    <span className="font-mono text-blue-400">{addr.address}/{addr.prefixlen}</span>
                  </div>
                ))}
                {iface.addresses?.filter(a => a.family === 'inet6' && a.scope === 'global').map((addr, i) => (
                  <div key={i} className="flex justify-between">
                    <span className="text-gray-400">IPv6</span>
                    <span className="font-mono text-purple-400 text-xs">{addr.address}</span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </Card>

      {/* Bridges */}
      <Card title="Bridges & Containers" icon={NetworkIcon}>
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-gray-400 border-b border-gray-700">
                <th className="pb-2">Interface</th>
                <th className="pb-2">État</th>
                <th className="pb-2">IPv4</th>
                <th className="pb-2">MTU</th>
              </tr>
            </thead>
            <tbody>
              {bridgeIfaces.map(iface => (
                <tr key={iface.name} className="border-b border-gray-700/50">
                  <td className="py-2 font-mono">{iface.name}</td>
                  <td className="py-2">
                    <StatusBadge status={iface.state === 'UP' ? 'up' : 'down'}>
                      {iface.state}
                    </StatusBadge>
                  </td>
                  <td className="py-2 font-mono text-blue-400">
                    {iface.addresses?.find(a => a.family === 'inet')?.address || '-'}
                  </td>
                  <td className="py-2 text-gray-400">{iface.mtu}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Routes */}
        <Card title="Table de Routage" icon={Route}>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-gray-400 border-b border-gray-700">
                  <th className="pb-2">Destination</th>
                  <th className="pb-2">Via</th>
                  <th className="pb-2">Interface</th>
                </tr>
              </thead>
              <tbody>
                {routes.slice(0, 15).map((route, i) => (
                  <tr key={i} className="border-b border-gray-700/50">
                    <td className="py-2 font-mono text-xs">
                      {route.destination === 'default' ? (
                        <span className="text-yellow-400">default</span>
                      ) : route.destination}
                    </td>
                    <td className="py-2 font-mono text-xs text-blue-400">
                      {route.gateway || '-'}
                    </td>
                    <td className="py-2 font-mono text-gray-400">{route.device}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>

        {/* NAT/Masquerade */}
        <Card title="NAT / Masquerade" icon={ArrowRight}>
          <div className="space-y-4">
            <div>
              <h4 className="text-sm font-semibold text-gray-400 mb-2">Masquerade</h4>
              <div className="space-y-2">
                {masquerade.map((rule, i) => (
                  <div key={i} className="bg-gray-900 rounded p-2 font-mono text-xs">
                    {rule.source} → {rule.outInterface || 'any'}
                    <span className="text-gray-500 ml-2">({rule.pkts} pkts)</span>
                  </div>
                ))}
              </div>
            </div>

            {forwards.length > 0 && (
              <div>
                <h4 className="text-sm font-semibold text-gray-400 mb-2">Port Forwards</h4>
                <div className="space-y-2">
                  {forwards.map((rule, i) => (
                    <div key={i} className="bg-gray-900 rounded p-2 font-mono text-xs">
                      :{rule.destinationPort} → {rule.forwardTo}
                      <span className="text-gray-500 ml-2">({rule.protocol})</span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        </Card>
      </div>
    </div>
  );
}

export default Network;
