import { useState } from 'react';

const statusConfig = {
  completed: {
    icon: (
      <svg className="w-3.5 h-3.5 text-green-500" viewBox="0 0 20 20" fill="currentColor">
        <path fillRule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clipRule="evenodd" />
      </svg>
    ),
    text: 'text-gray-500 line-through',
  },
  in_progress: {
    icon: (
      <svg className="w-3.5 h-3.5 text-purple-400 animate-spin" viewBox="0 0 24 24" fill="none">
        <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="3" />
        <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
      </svg>
    ),
    text: 'text-purple-300',
  },
  pending: {
    icon: (
      <svg className="w-3.5 h-3.5 text-gray-600" viewBox="0 0 20 20" fill="none" stroke="currentColor">
        <circle cx="10" cy="10" r="7" strokeWidth="1.5" />
      </svg>
    ),
    text: 'text-gray-500',
  },
};

export default function TodoPanel({ todos }) {
  const [collapsed, setCollapsed] = useState(false);

  if (!todos || todos.length === 0) return null;

  const completed = todos.filter(t => t.status === 'completed').length;
  const total = todos.length;
  const allDone = completed === total;
  const activeTask = todos.find(t => t.status === 'in_progress');

  return (
    <div className="border-b border-gray-800 bg-gray-900/50">
      {/* Header bar */}
      <button
        onClick={() => setCollapsed(c => !c)}
        className="w-full flex items-center gap-2 px-4 py-1.5 text-xs hover:bg-gray-800/50 transition-colors"
      >
        <svg
          className={`w-3 h-3 text-gray-500 transition-transform ${collapsed ? '' : 'rotate-90'}`}
          viewBox="0 0 20 20"
          fill="currentColor"
        >
          <path fillRule="evenodd" d="M7.21 14.77a.75.75 0 01.02-1.06L11.168 10 7.23 6.29a.75.75 0 111.04-1.08l4.5 4.25a.75.75 0 010 1.08l-4.5 4.25a.75.75 0 01-1.06-.02z" clipRule="evenodd" />
        </svg>
        <span className={`font-medium ${allDone ? 'text-green-500' : 'text-gray-400'}`}>
          Tasks
        </span>
        <span className="text-gray-600">
          {completed}/{total}
        </span>
        {/* Progress bar */}
        <div className="flex-1 max-w-[120px] h-1 bg-gray-800 rounded-full overflow-hidden">
          <div
            className={`h-full transition-all duration-300 rounded-full ${allDone ? 'bg-green-600' : 'bg-purple-600'}`}
            style={{ width: `${(completed / total) * 100}%` }}
          />
        </div>
        {activeTask && !collapsed && (
          <span className="text-purple-400 truncate ml-1">
            {activeTask.activeForm || activeTask.content}
          </span>
        )}
      </button>

      {/* Todo items */}
      {!collapsed && (
        <div className="px-4 pb-2 space-y-0.5">
          {todos.map((todo, i) => {
            const cfg = statusConfig[todo.status] || statusConfig.pending;
            return (
              <div key={i} className="flex items-center gap-2 py-0.5">
                <span className="flex-shrink-0">{cfg.icon}</span>
                <span className={`text-xs truncate ${cfg.text}`}>
                  {todo.status === 'in_progress'
                    ? (todo.activeForm || todo.content)
                    : todo.content}
                </span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
