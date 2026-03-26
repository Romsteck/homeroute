import { useState } from 'react';

const statusConfig = {
  completed: {
    icon: (
      <svg className="w-4 h-4 text-green-500" viewBox="0 0 20 20" fill="currentColor">
        <path fillRule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clipRule="evenodd" />
      </svg>
    ),
    text: 'text-gray-500 line-through',
  },
  in_progress: {
    icon: (
      <svg className="w-4 h-4 text-purple-400 animate-spin" viewBox="0 0 24 24" fill="none">
        <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="3" />
        <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
      </svg>
    ),
    text: 'text-purple-300 font-medium',
  },
  pending: {
    icon: (
      <svg className="w-4 h-4 text-gray-600" viewBox="0 0 20 20" fill="none" stroke="currentColor">
        <circle cx="10" cy="10" r="7" strokeWidth="1.5" />
      </svg>
    ),
    text: 'text-gray-500',
  },
};

function groupIntoPhases(todos) {
  const groups = [];
  let currentGroup = { phase: null, todos: [] };

  for (const todo of todos) {
    if (todo.content && todo.content.startsWith('▸ Phase')) {
      if (currentGroup.phase || currentGroup.todos.length > 0) {
        groups.push(currentGroup);
      }
      currentGroup = { phase: todo, todos: [] };
    } else {
      currentGroup.todos.push(todo);
    }
  }
  if (currentGroup.phase || currentGroup.todos.length > 0) {
    groups.push(currentGroup);
  }
  return groups;
}

function TodoItem({ todo }) {
  const cfg = statusConfig[todo.status] || statusConfig.pending;
  return (
    <div className="flex items-center gap-2.5 py-0.5">
      <span className="flex-shrink-0">{cfg.icon}</span>
      <span className={`text-sm truncate ${cfg.text}`}>
        {todo.status === 'in_progress'
          ? (todo.activeForm || todo.content)
          : todo.content}
      </span>
    </div>
  );
}

function PhaseHeader({ phase }) {
  const phaseName = phase.content.replace(/^▸\s*/, '');
  const statusColor = phase.status === 'completed'
    ? 'text-green-500'
    : phase.status === 'in_progress'
      ? 'text-purple-400'
      : 'text-gray-600';

  return (
    <div className={`flex items-center gap-2 text-sm font-medium ${statusColor}`}>
      <span className="text-xs">▸</span>
      <span>{phaseName}</span>
    </div>
  );
}

function PhaseProgress({ todos }) {
  const completed = todos.filter(t => t.status === 'completed').length;
  const total = todos.length;
  if (total === 0) return null;
  const allDone = completed === total;

  return (
    <div className="flex items-center gap-2 ml-5">
      <span className="text-xs text-gray-600">{completed}/{total}</span>
      <div className="w-16 h-1 bg-gray-800 rounded-full overflow-hidden">
        <div
          className={`h-full transition-all duration-300 rounded-full ${allDone ? 'bg-green-600' : 'bg-purple-600/60'}`}
          style={{ width: `${(completed / total) * 100}%` }}
        />
      </div>
    </div>
  );
}

export default function TodoPanel({ todos }) {
  const [collapsed, setCollapsed] = useState(false);

  if (!todos || todos.length === 0) return null;

  const groups = groupIntoPhases(todos);
  const hasPhases = groups.some(g => g.phase !== null);

  // Count only real tasks (exclude phase headers)
  const realTodos = todos.filter(t => !t.content?.startsWith('▸ Phase'));
  const completed = realTodos.filter(t => t.status === 'completed').length;
  const total = realTodos.length;
  const allDone = completed === total;
  const activeTask = realTodos.find(t => t.status === 'in_progress');

  return (
    <div className={`relative bg-gray-900/80 shadow-lg shadow-black/30 ${allDone ? 'border-b border-green-900/30' : 'border-b border-purple-900/40'}`}>
      {/* Header bar */}
      <button
        onClick={() => setCollapsed(c => !c)}
        className="w-full flex items-center gap-2.5 px-4 py-2.5 text-sm hover:bg-gray-800/50 transition-colors"
      >
        <svg
          className={`w-3 h-3 text-gray-500 transition-transform ${collapsed ? '' : 'rotate-90'}`}
          viewBox="0 0 20 20"
          fill="currentColor"
        >
          <path fillRule="evenodd" d="M7.21 14.77a.75.75 0 01.02-1.06L11.168 10 7.23 6.29a.75.75 0 111.04-1.08l4.5 4.25a.75.75 0 010 1.08l-4.5 4.25a.75.75 0 01-1.06-.02z" clipRule="evenodd" />
        </svg>
        <span className={`font-medium ${allDone ? 'text-green-500' : 'text-gray-300'}`}>
          Tasks
        </span>
        <span className="text-gray-600">
          {completed}/{total}
        </span>
        {/* Progress bar */}
        <div className="flex-1 max-w-[140px] h-1.5 bg-gray-800 rounded-full overflow-hidden">
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
        <div className="px-4 pb-3">
          {hasPhases ? (
            // Phased rendering
            <div className="space-y-2.5">
              {groups.map((group, gi) => (
                <div key={gi}>
                  {group.phase && (
                    <div className="mb-1">
                      <PhaseHeader phase={group.phase} />
                      <PhaseProgress todos={group.todos} />
                    </div>
                  )}
                  <div className="space-y-0.5 ml-3">
                    {group.todos.map((todo, ti) => (
                      <TodoItem key={ti} todo={todo} />
                    ))}
                  </div>
                </div>
              ))}
            </div>
          ) : (
            // Flat rendering (backward compatible)
            <div className="space-y-1">
              {todos.map((todo, i) => (
                <TodoItem key={i} todo={todo} />
              ))}
            </div>
          )}
        </div>
      )}

      {/* Soft downward shadow bleed */}
      <div className="absolute bottom-0 left-0 right-0 h-3 bg-gradient-to-b from-black/20 to-transparent translate-y-full pointer-events-none z-10" />
    </div>
  );
}
