import { NavLink, useLocation } from 'react-router-dom';
import {
  LayoutDashboard, Server, Shield, Globe, Settings,
  ArrowLeftRight, RefreshCw, LogOut, Activity,
  User, HardDrive, Lock, Boxes, Code2, Database,
  Store as StoreIcon, GitBranch, Archive, X, ListTodo, Zap, ExternalLink, ScrollText, TableProperties
} from 'lucide-react';
import { useAuth } from '../context/AuthContext';
import { useEffect, useState } from 'react';
import { getUpdateCount } from '../api/client';

const navGroups = [
  {
    items: [
      { to: '/', icon: LayoutDashboard, label: 'Dashboard' },
    ],
  },
  {
    label: 'Réseau',
    items: [
      { to: '/dns', icon: Server, label: 'DNS / DHCP' },
      { to: '/adblock', icon: Shield, label: 'AdBlock' },
      { to: '/ddns', icon: Globe, label: 'Dynamic DNS' },
    ],
  },
  {
    label: 'Services',
    items: [
      { to: '/reverseproxy', icon: ArrowLeftRight, label: 'Reverse Proxy' },
      { to: '/certificates', icon: Lock, label: 'Certificats' },
    ],
  },
  {
    label: 'Applications',
    items: [
      { to: '/studio', icon: Code2, label: 'Studio', highlight: true },
      { to: '/database', icon: Database, label: 'Base de donnees' },
      { to: '/schema', icon: TableProperties, label: 'Schema' },
      { to: '/store', icon: StoreIcon, label: 'Store' },
      { to: '/git', icon: GitBranch, label: 'Git' },
    ],
  },
  {
    label: 'Système',
    items: [
      { to: '/monitoring', icon: Activity, label: 'Monitoring' },
      { to: '/hosts', icon: HardDrive, label: 'Hotes' },
      { to: '/updates', icon: RefreshCw, label: 'Mises à jour' },
      { to: '/energy', icon: Zap, label: 'Energie' },
      { to: '/backup', icon: Archive, label: 'Backup' },
      { to: '/logs', icon: ScrollText, label: 'Logs' },
    ],
  },
];

function Sidebar({ onClose }) {
  const { user, logout } = useAuth();
  const location = useLocation();
  const [updateCount, setUpdateCount] = useState(0);

  // Auto-close sidebar on route change (mobile)
  useEffect(() => {
    if (onClose) onClose();
  }, [location.pathname]);

  // Poll update count every 60s
  useEffect(() => {
    const fetch = () => {
      getUpdateCount().then(r => {
        if (typeof r.data?.count === 'number') setUpdateCount(r.data.count);
      }).catch(() => {});
    };
    fetch();
    const timer = setInterval(fetch, 60000);
    return () => clearInterval(timer);
  }, []);

  return (
    <aside className="w-64 h-full bg-gray-800 border-r border-gray-700 flex flex-col">
      <div className="p-4 border-b border-gray-700 flex items-center justify-between">
        <h1 className="text-xl font-bold flex items-center gap-2">
          <Settings className="w-6 h-6 text-blue-400" />
          HomeRoute
        </h1>
        {onClose && (
          <button
            onClick={onClose}
            className="lg:hidden p-1 text-gray-400 hover:text-white"
          >
            <X className="w-5 h-5" />
          </button>
        )}
      </div>

      <nav className="flex-1 py-2 overflow-y-auto">
        {navGroups.map((group, gi) => (
          <div key={gi}>
            {group.label && (
              <div className="px-4 pt-4 pb-1 text-xs text-gray-500 uppercase tracking-wider">
                {group.label}
              </div>
            )}
            <ul className="space-y-0.5">
              {group.items.map((item) => {
                const { icon: Icon, label, highlight, external, href, to } = item;
                if (external) {
                  return (
                    <li key={href}>
                      <a
                        href={href}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="flex items-center gap-3 px-4 py-2 transition-[background-color,color] duration-300 ease-out hover:duration-0 text-sm border-l-3 border-transparent text-gray-300 hover:bg-gray-700/30"
                      >
                        <Icon className="w-5 h-5" />
                        <span className="flex-1">{label}</span>
                        <ExternalLink className="w-3.5 h-3.5 text-gray-500" />
                      </a>
                    </li>
                  );
                }
                return (
                  <li key={to}>
                    <NavLink
                      to={to}
                      className={({ isActive }) =>
                        `flex items-center gap-3 px-4 py-2 transition-[background-color,color] duration-300 ease-out hover:duration-0 text-sm ${
                          isActive
                            ? 'border-l-3 border-blue-400 bg-gray-700/50 text-white'
                            : 'border-l-3 border-transparent text-gray-300 hover:bg-gray-700/30'
                        }`
                      }
                    >
                      <Icon className={`w-5 h-5${highlight ? ' text-blue-400' : ''}`} />
                      <span className="flex-1">{label}</span>
                      {to === '/updates' && updateCount > 0 && (
                        <span className="ml-auto bg-red-500 text-white text-xs font-bold rounded-full min-w-[20px] h-5 flex items-center justify-center px-1.5">
                          {updateCount}
                        </span>
                      )}
                    </NavLink>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </nav>

      <div className="p-4 border-t border-gray-700">
        {user && (
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2 min-w-0">
              <User className="w-4 h-4 text-gray-400 flex-shrink-0" />
              <div className="min-w-0">
                <p className="text-sm text-gray-300 truncate">
                  {user.displayName || user.username}
                </p>
                <p className="text-xs text-blue-400">Admin</p>
              </div>
            </div>
            <button
              onClick={logout}
              className="p-2 text-gray-400 hover:text-red-400 hover:bg-gray-700 transition-[background-color,color] duration-300 ease-out hover:duration-0"
              title="Deconnexion"
            >
              <LogOut className="w-4 h-4" />
            </button>
          </div>
        )}
        <p className="text-xs text-gray-500 mt-2">HomeRoute</p>
      </div>
    </aside>
  );
}

export default Sidebar;
