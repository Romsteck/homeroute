import { useState, useEffect } from 'react';
import { Link } from 'react-router-dom';
import { LayoutDashboard, Shield, Globe, Wifi, ArrowRight } from 'lucide-react';
import ServiceStatusPanel from '../components/ServiceStatusPanel';
import PageHeader from '../components/PageHeader';
import Section from '../components/Section';
import { getDhcpLeases, getAdblockStats, getDdnsStatus, getServicesStatus } from '../api/client';

function Dashboard() {
  const [data, setData] = useState({
    leases: null,
    adblock: null,
    ddns: null,
    services: null,
    loading: true
  });

  useEffect(() => {
    async function fetchData() {
      try {
        const [leaseRes, adblockRes, ddnsRes, svcRes] = await Promise.all([
          getDhcpLeases(),
          getAdblockStats(),
          getDdnsStatus(),
          getServicesStatus()
        ]);

        setData({
          leases: leaseRes.data.success ? leaseRes.data.leases : [],
          adblock: adblockRes.data.success ? adblockRes.data.stats : null,
          ddns: ddnsRes.data.success ? ddnsRes.data.status : null,
          services: svcRes.data.success ? svcRes.data.services : [],
          loading: false
        });
      } catch (error) {
        console.error('Error fetching data:', error);
        setData(prev => ({ ...prev, loading: false }));
      }
    }

    fetchData();
    const interval = setInterval(fetchData, 30000);
    return () => clearInterval(interval);
  }, []);

  if (data.loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-400"></div>
      </div>
    );
  }

  return (
    <div>
      <PageHeader title="Dashboard" icon={LayoutDashboard} />

      <Section title="Vue d'ensemble" flush>
        <div className="flex flex-col sm:flex-row sm:items-stretch divide-y sm:divide-y-0 sm:divide-x divide-gray-700">
          <Link to="/dns" className="flex items-center gap-3 px-4 py-3 flex-1 hover:bg-gray-800/50">
            <Wifi className="w-4 h-4 text-blue-400 shrink-0" />
            <span className="text-xs text-gray-500 uppercase">DHCP</span>
            <span className="text-lg font-bold text-blue-400">{data.leases?.length || 0}</span>
            <span className="text-xs text-gray-500">appareils</span>
            <ArrowRight className="w-3.5 h-3.5 text-gray-600 ml-auto shrink-0" />
          </Link>
          <Link to="/adblock" className="flex items-center gap-3 px-4 py-3 flex-1 hover:bg-gray-800/50">
            <Shield className="w-4 h-4 text-green-400 shrink-0" />
            <span className="text-xs text-gray-500 uppercase">AdBlock</span>
            <span className="text-lg font-bold text-green-400">{data.adblock?.domainCount?.toLocaleString() || 0}</span>
            <span className="text-xs text-gray-500">bloqués</span>
            <ArrowRight className="w-3.5 h-3.5 text-gray-600 ml-auto shrink-0" />
          </Link>
          <Link to="/ddns" className="flex items-center gap-3 px-4 py-3 flex-1 hover:bg-gray-800/50">
            <Globe className="w-4 h-4 text-blue-400 shrink-0" />
            <span className="text-xs text-gray-500 uppercase">DDNS</span>
            <span className="text-sm font-mono text-blue-400 truncate">{data.ddns?.config?.recordName || '-'}</span>
            <ArrowRight className="w-3.5 h-3.5 text-gray-600 ml-auto shrink-0" />
          </Link>
        </div>
      </Section>

      <Section title="Services" contrast flush>
        <ServiceStatusPanel services={data.services} />
      </Section>

      <Section title="Baux DHCP Récents" flush>
        <div className="overflow-x-auto">
          <table className="w-full text-sm min-w-[500px]">
            <thead>
              <tr className="text-left text-gray-400 border-b border-gray-700">
                <th className="px-4 pb-1">Hostname</th>
                <th className="px-4 pb-1">IP</th>
                <th className="px-4 pb-1 hidden sm:table-cell">MAC</th>
                <th className="px-4 pb-1 hidden sm:table-cell">Expiration</th>
              </tr>
            </thead>
            <tbody>
              {data.leases?.slice(0, 10).map(lease => (
                <tr key={lease.mac} className="border-b border-gray-700/50">
                  <td className="px-4 py-1 font-mono">
                    {lease.hostname || <span className="text-gray-500">-</span>}
                  </td>
                  <td className="px-4 py-1 font-mono text-blue-400">{lease.ip}</td>
                  <td className="px-4 py-1 font-mono text-gray-400 text-xs hidden sm:table-cell">{lease.mac}</td>
                  <td className="px-4 py-1 text-gray-400 text-xs hidden sm:table-cell">
                    {new Date(lease.expiration).toLocaleString('fr-FR')}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Section>
    </div>
  );
}

export default Dashboard;
