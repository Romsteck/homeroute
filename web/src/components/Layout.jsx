import { useState } from "react";
import Sidebar from "./Sidebar";
import { Menu } from "lucide-react";
import TaskBell from "./tasks/TaskBell";
import TaskDropdown from "./tasks/TaskDropdown";

function Layout({ children }) {
  const [sidebarOpen, setSidebarOpen] = useState(false);

  return (
    <div className="flex h-screen">
      {/* Mobile backdrop */}
      {sidebarOpen && (
        <div
          className="fixed inset-0 bg-black/60 z-40 lg:hidden"
          onClick={() => setSidebarOpen(false)}
        />
      )}

      {/* Sidebar */}
      <div
        className={`fixed inset-y-0 left-0 z-50 w-64 transform transition-transform duration-200 ease-out lg:relative lg:translate-x-0 ${
          sidebarOpen ? "translate-x-0" : "-translate-x-full"
        }`}
      >
        <Sidebar onClose={() => setSidebarOpen(false)} />
      </div>

      {/* Main content */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Top bar */}
        <div className="flex items-center justify-between px-4 py-2 bg-gray-800 border-b border-gray-700">
          <div className="flex items-center gap-3">
            <button
              onClick={() => setSidebarOpen(true)}
              className="lg:hidden p-1 text-gray-400 hover:text-white"
            >
              <Menu className="w-6 h-6" />
            </button>
            <h1 className="lg:hidden text-lg font-bold">HomeRoute</h1>
          </div>
          <div className="relative">
            <TaskBell />
            <TaskDropdown />
          </div>
        </div>
        <main className="flex-1 overflow-auto">{children}</main>
      </div>
    </div>
  );
}

export default Layout;
