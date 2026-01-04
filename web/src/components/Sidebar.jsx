import { NavLink } from 'react-router-dom';
import {
  LayoutDashboard,
  Server,
  Network,
  Shield,
  Globe,
  Settings,
  HardDrive,
  ArrowLeftRight,
  FolderOpen,
  RefreshCw,
  Zap
} from 'lucide-react';

const navItems = [
  { to: '/', icon: LayoutDashboard, label: 'Dashboard' },
  { to: '/dns', icon: Server, label: 'DNS / DHCP' },
  { to: '/network', icon: Network, label: 'Réseau' },
  { to: '/adblock', icon: Shield, label: 'AdBlock' },
  { to: '/ddns', icon: Globe, label: 'Dynamic DNS' },
  { to: '/backup', icon: HardDrive, label: 'Backup' },
  { to: '/reverseproxy', icon: ArrowLeftRight, label: 'Reverse Proxy' },
  { to: '/samba', icon: FolderOpen, label: 'Samba' },
  { to: '/updates', icon: RefreshCw, label: 'Mises a jour' },
  { to: '/energy', icon: Zap, label: 'Énergie' },
];

function Sidebar() {
  return (
    <aside className="w-64 bg-gray-800 border-r border-gray-700 flex flex-col">
      <div className="p-4 border-b border-gray-700">
        <h1 className="text-xl font-bold flex items-center gap-2">
          <Settings className="w-6 h-6 text-blue-400" />
          Server Dashboard
        </h1>
        <p className="text-xs text-gray-400 mt-1">cloudmaster</p>
      </div>

      <nav className="flex-1 p-4">
        <ul className="space-y-2">
          {navItems.map(({ to, icon: Icon, label }) => (
            <li key={to}>
              <NavLink
                to={to}
                className={({ isActive }) =>
                  `flex items-center gap-3 px-4 py-2 rounded-lg transition-colors ${
                    isActive
                      ? 'bg-blue-600 text-white'
                      : 'text-gray-300 hover:bg-gray-700'
                  }`
                }
              >
                <Icon className="w-5 h-5" />
                {label}
              </NavLink>
            </li>
          ))}
        </ul>
      </nav>

      <div className="p-4 border-t border-gray-700 text-xs text-gray-500">
        mynetwk.biz
      </div>
    </aside>
  );
}

export default Sidebar;
