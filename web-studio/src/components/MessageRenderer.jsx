import { useState } from 'react';

function ChevronIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
    </svg>
  );
}

function renderMarkdown(text) {
  if (!text) return null;
  const parts = [];
  const lines = text.split('\n');
  let i = 0;

  while (i < lines.length) {
    if (lines[i].startsWith('```')) {
      const lang = lines[i].slice(3).trim();
      const codeLines = [];
      i++;
      while (i < lines.length && !lines[i].startsWith('```')) {
        codeLines.push(lines[i]);
        i++;
      }
      i++;
      parts.push(
        <div key={parts.length} className="my-2">
          {lang && (
            <div className="bg-gray-950 border border-gray-700 border-b-0 rounded-t-lg px-3 py-1 text-xs text-gray-500 font-mono">
              {lang}
            </div>
          )}
          <pre className={`bg-gray-950 border border-gray-700 ${lang ? 'rounded-b-lg' : 'rounded-lg'} p-3 overflow-x-auto text-[13px] font-mono`}>
            <code className="text-gray-300">{codeLines.join('\n')}</code>
          </pre>
        </div>
      );
    } else if (lines[i].startsWith('- ') || lines[i].startsWith('* ')) {
      const listItems = [];
      while (i < lines.length && (lines[i].startsWith('- ') || lines[i].startsWith('* '))) {
        listItems.push(lines[i].slice(2));
        i++;
      }
      parts.push(
        <ul key={parts.length} className="list-disc list-inside space-y-1 my-1 ml-2">
          {listItems.map((item, j) => (
            <li key={j}>{renderInline(item)}</li>
          ))}
        </ul>
      );
    } else if (/^\d+\.\s/.test(lines[i])) {
      const listItems = [];
      while (i < lines.length && /^\d+\.\s/.test(lines[i])) {
        listItems.push(lines[i].replace(/^\d+\.\s/, ''));
        i++;
      }
      parts.push(
        <ol key={parts.length} className="list-decimal list-inside space-y-1 my-1 ml-2">
          {listItems.map((item, j) => (
            <li key={j}>{renderInline(item)}</li>
          ))}
        </ol>
      );
    } else {
      parts.push(
        <span key={parts.length}>
          {renderInline(lines[i])}
          {i < lines.length - 1 ? '\n' : ''}
        </span>
      );
      i++;
    }
  }
  return parts;
}

function renderInline(line) {
  const parts = [];
  const regex = /(\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\(([^)]+)\))/g;
  let lastIndex = 0;
  let match;

  while ((match = regex.exec(line)) !== null) {
    if (match.index > lastIndex) {
      parts.push(line.slice(lastIndex, match.index));
    }
    if (match[2]) {
      parts.push(<strong key={parts.length} className="text-gray-100 font-semibold">{match[2]}</strong>);
    } else if (match[3]) {
      parts.push(
        <code key={parts.length} className="bg-gray-800 px-1.5 py-0.5 rounded text-[13px] font-mono text-gray-300">{match[3]}</code>
      );
    } else if (match[4] && match[5]) {
      parts.push(
        <a key={parts.length} href={match[5]} className="text-indigo-400 hover:underline" target="_blank" rel="noopener noreferrer">{match[4]}</a>
      );
    }
    lastIndex = regex.lastIndex;
  }
  if (lastIndex < line.length) {
    parts.push(line.slice(lastIndex));
  }
  return parts;
}

function ToolUseMessage({ message }) {
  const [expanded, setExpanded] = useState(false);
  const inputStr = message.input ? JSON.stringify(message.input, null, 2) : '';

  // Generate a brief description from the input
  let brief = '';
  if (message.input) {
    if (message.input.file_path) brief = message.input.file_path.split('/').pop();
    else if (message.input.command) {
      const cmd = message.input.command;
      brief = cmd.length > 50 ? cmd.slice(0, 50) + '...' : cmd;
    }
    else if (message.input.pattern) brief = message.input.pattern;
  }

  return (
    <div className="mb-2 ml-2">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 text-xs text-purple-400 hover:text-purple-300 transition-colors"
      >
        <ChevronIcon className={`w-3 h-3 transition-transform ${expanded ? 'rotate-90' : ''}`} />
        <span className="font-medium">{message.tool}</span>
        {message.status === 'success' && <span className="text-green-500 text-xs font-bold">&#10003;</span>}
        {message.status === 'error' && <span className="text-red-400 text-xs font-bold">&#10007;</span>}
        {brief && <span className="text-gray-600 font-mono truncate max-w-[300px]">{brief}</span>}
      </button>
      {expanded && inputStr && (
        <pre className="ml-5 mt-1 text-xs text-gray-500 bg-gray-800/50 rounded p-2 overflow-x-auto max-h-48 overflow-y-auto font-mono">
          {inputStr}
        </pre>
      )}
    </div>
  );
}

function ToolResultMessage({ message }) {
  const [expanded, setExpanded] = useState(false);
  const content = typeof message.content === 'string' ? message.content : JSON.stringify(message.content, null, 2);

  return (
    <div className="mb-3 ml-2">
      <button
        onClick={() => setExpanded(!expanded)}
        className={`flex items-center gap-2 text-xs transition-colors ${
          message.is_error ? 'text-red-400/70 hover:text-red-400' : 'text-green-500/70 hover:text-green-500'
        }`}
      >
        <ChevronIcon className={`w-3 h-3 transition-transform ${expanded ? 'rotate-90' : ''}`} />
        <span>Result</span>
        {message.is_error && <span className="text-red-400">Error</span>}
      </button>
      {expanded && content && (
        <pre className="ml-5 mt-1 text-xs text-gray-500 bg-gray-800/30 rounded p-2 overflow-x-auto max-h-48 overflow-y-auto font-mono whitespace-pre-wrap">
          {content}
        </pre>
      )}
    </div>
  );
}

function ThinkingMessage({ message }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="mb-2 ml-2">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 text-xs text-gray-500 hover:text-gray-400 transition-colors"
      >
        <ChevronIcon className={`w-3 h-3 transition-transform ${expanded ? 'rotate-90' : ''}`} />
        <span className="italic">Thinking...</span>
      </button>
      {expanded && (
        <pre className="ml-5 mt-1 text-xs text-gray-500 italic bg-gray-800/30 rounded p-2 overflow-x-auto max-h-48 overflow-y-auto whitespace-pre-wrap font-mono">
          {message.content}
        </pre>
      )}
    </div>
  );
}

export default function MessageRenderer({ message, onSend }) {
  switch (message.type) {
    case 'human':
      return (
        <div className="flex justify-end mb-4">
          <div className="max-w-[85%] bg-indigo-600/20 border border-indigo-500/30 rounded-2xl px-4 py-3">
            <p className="text-gray-200 text-sm leading-relaxed whitespace-pre-wrap">{message.content}</p>
          </div>
        </div>
      );

    case 'assistant':
      return (
        <div className="flex justify-start mb-4">
          <div className="max-w-[85%]">
            <div className="text-gray-300 text-sm leading-relaxed prose-studio whitespace-pre-wrap">
              {renderMarkdown(message.content)}
            </div>
          </div>
        </div>
      );

    case 'tool_use':
      return <ToolUseMessage message={message} />;

    case 'tool_result':
      return <ToolResultMessage message={message} />;

    case 'thinking':
      return <ThinkingMessage message={message} />;

    case 'error':
      return (
        <div className="flex justify-start mb-4">
          <div className="max-w-[85%] bg-red-900/20 border border-red-500/30 rounded-2xl px-4 py-3 text-red-300 text-sm">
            {message.content}
          </div>
        </div>
      );

    case 'plan_complete':
      return (
        <div className="flex justify-center my-6">
          <div className="flex items-center gap-3 bg-amber-950/30 border border-amber-500/30 rounded-2xl px-5 py-3">
            <svg className="w-4 h-4 text-amber-400 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <span className="text-sm text-amber-300 font-medium">Plan complete</span>
            <button
              onClick={() => onSend && onSend('Proceed with the plan. Execute all proposed changes.', 'default')}
              className="px-4 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium rounded-xl transition-colors"
            >
              Execute
            </button>
            <button
              onClick={() => onSend && onSend('Please revise the plan with my additional feedback.', 'plan')}
              className="px-4 py-1.5 bg-gray-700 hover:bg-gray-600 text-gray-300 text-sm font-medium rounded-xl transition-colors"
            >
              Refine
            </button>
          </div>
        </div>
      );

    case 'raw':
      return (
        <div className="text-gray-600 text-xs font-mono ml-2 mb-2">
          {JSON.stringify(message.data)}
        </div>
      );

    default:
      return null;
  }
}
