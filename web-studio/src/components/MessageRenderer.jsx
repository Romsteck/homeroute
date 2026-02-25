import { useState, useEffect } from 'react';

function ChevronIcon({ className }) {
  return (
    <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
    </svg>
  );
}

// --- Markdown rendering ---

function renderInline(line) {
  const parts = [];
  // Order matters: ** before *, ~~ before single chars
  const regex = /(\*\*(.+?)\*\*|~~(.+?)~~|\*(.+?)\*|_(.+?)_|`([^`]+)`|\[([^\]]+)\]\(([^)]+)\))/g;
  let lastIndex = 0;
  let match;

  while ((match = regex.exec(line)) !== null) {
    if (match.index > lastIndex) {
      parts.push(line.slice(lastIndex, match.index));
    }
    if (match[2]) {
      // Bold **text**
      parts.push(<strong key={parts.length} className="text-gray-100 font-semibold">{match[2]}</strong>);
    } else if (match[3]) {
      // Strikethrough ~~text~~
      parts.push(<del key={parts.length} className="text-gray-500">{match[3]}</del>);
    } else if (match[4]) {
      // Italic *text*
      parts.push(<em key={parts.length} className="text-gray-300 italic">{match[4]}</em>);
    } else if (match[5]) {
      // Italic _text_
      parts.push(<em key={parts.length} className="text-gray-300 italic">{match[5]}</em>);
    } else if (match[6]) {
      // Inline code `code`
      parts.push(
        <code key={parts.length} className="bg-gray-800 px-1.5 py-0.5 rounded text-[13px] font-mono text-gray-300">{match[6]}</code>
      );
    } else if (match[7] && match[8]) {
      // Link [text](url)
      parts.push(
        <a key={parts.length} href={match[8]} className="text-indigo-400 hover:underline" target="_blank" rel="noopener noreferrer">{match[7]}</a>
      );
    }
    lastIndex = regex.lastIndex;
  }
  if (lastIndex < line.length) {
    parts.push(line.slice(lastIndex));
  }
  return parts;
}

function renderMarkdown(text) {
  if (!text) return null;
  const parts = [];
  const lines = text.split('\n');
  let i = 0;

  while (i < lines.length) {
    // Empty lines → spacer
    if (lines[i].trim() === '') {
      parts.push(<div key={parts.length} className="h-2" />);
      i++;
      continue;
    }

    // Code blocks ```
    if (lines[i].startsWith('```')) {
      const lang = lines[i].slice(3).trim();
      const codeLines = [];
      i++;
      while (i < lines.length && !lines[i].startsWith('```')) {
        codeLines.push(lines[i]);
        i++;
      }
      if (i < lines.length) i++; // skip closing ```
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
      continue;
    }

    // Headings # ## ### ####
    const headingMatch = lines[i].match(/^(#{1,4})\s+(.+)$/);
    if (headingMatch) {
      const level = headingMatch[1].length;
      const headingText = headingMatch[2];
      const sizes = {
        1: 'text-xl font-bold',
        2: 'text-lg font-semibold',
        3: 'text-base font-semibold',
        4: 'text-sm font-semibold',
      };
      parts.push(
        <div key={parts.length} className={`${sizes[level]} text-gray-100 mt-4 mb-2`}>
          {renderInline(headingText)}
        </div>
      );
      i++;
      continue;
    }

    // Horizontal rules --- *** ___
    if (/^(-{3,}|\*{3,}|_{3,})\s*$/.test(lines[i])) {
      parts.push(<hr key={parts.length} className="border-gray-700 my-4" />);
      i++;
      continue;
    }

    // Blockquotes > text
    if (lines[i].startsWith('> ')) {
      const quoteLines = [];
      while (i < lines.length && lines[i].startsWith('> ')) {
        quoteLines.push(lines[i].slice(2));
        i++;
      }
      parts.push(
        <blockquote key={parts.length} className="border-l-2 border-gray-600 pl-3 my-2 text-gray-400 italic">
          {renderMarkdown(quoteLines.join('\n'))}
        </blockquote>
      );
      continue;
    }

    // Tables | col | col |
    if (lines[i].startsWith('|') && lines[i].includes('|', 1)) {
      const tableLines = [];
      while (i < lines.length && lines[i].startsWith('|')) {
        tableLines.push(lines[i]);
        i++;
      }
      if (tableLines.length >= 2) {
        const parseRow = (line) => line.split('|').slice(1, -1).map(c => c.trim());
        const headers = parseRow(tableLines[0]);
        // Skip separator line (|---|---|), take rest as rows
        const startRow = (tableLines.length > 1 && /^[\s|:-]+$/.test(tableLines[1])) ? 2 : 1;
        const rows = tableLines.slice(startRow).map(parseRow);
        parts.push(
          <div key={parts.length} className="my-2 overflow-x-auto">
            <table className="text-sm border-collapse">
              <thead>
                <tr>
                  {headers.map((h, j) => (
                    <th key={j} className="border border-gray-700 px-3 py-1.5 text-left text-gray-300 bg-gray-800/50 font-medium">
                      {renderInline(h)}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.map((row, ri) => (
                  <tr key={ri}>
                    {row.map((cell, ci) => (
                      <td key={ci} className="border border-gray-700 px-3 py-1.5 text-gray-400">
                        {renderInline(cell)}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        );
        continue;
      }
    }

    // Bullet lists (- or *)
    if (lines[i].startsWith('- ') || lines[i].startsWith('* ')) {
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
      continue;
    }

    // Numbered lists
    if (/^\d+\.\s/.test(lines[i])) {
      const startNum = parseInt(lines[i].match(/^(\d+)\./)[1], 10);
      const listItems = [];
      while (i < lines.length && /^\d+\.\s/.test(lines[i])) {
        listItems.push(lines[i].replace(/^\d+\.\s/, ''));
        i++;
      }
      parts.push(
        <ol key={parts.length} start={startNum} className="list-decimal list-inside space-y-1 my-1 ml-2">
          {listItems.map((item, j) => (
            <li key={j}>{renderInline(item)}</li>
          ))}
        </ol>
      );
      continue;
    }

    // Regular paragraph line
    parts.push(
      <p key={parts.length} className="my-0.5">
        {renderInline(lines[i])}
      </p>
    );
    i++;
  }
  return parts;
}

// --- Tool use / result messages ---

function ToolUseMessage({ message }) {
  const [expanded, setExpanded] = useState(false);
  const inputStr = message.input ? JSON.stringify(message.input, null, 2) : '';

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

// --- AskUserQuestion interactive widget ---

function AskUserQuestionWidget({ message, pendingAnswersRef }) {
  const { questions } = message;
  const [answers, setAnswers] = useState(() => {
    const init = {};
    (questions || []).forEach((q, i) => {
      init[i] = { question: q.question, selected: [], other: '' };
    });
    return init;
  });

  // Sync answers to ref keyed by tool_use_id
  useEffect(() => {
    if (pendingAnswersRef && message.tool_use_id) {
      pendingAnswersRef.current[message.tool_use_id] = answers;
    }
  }, [answers, pendingAnswersRef, message.tool_use_id]);

  if (!questions || questions.length === 0) return null;

  function toggleOption(qIdx, label, multiSelect) {
    setAnswers(prev => {
      const q = { ...prev[qIdx] };
      if (multiSelect) {
        if (q.selected.includes(label)) {
          q.selected = q.selected.filter(s => s !== label);
        } else {
          q.selected = [...q.selected, label];
        }
      } else {
        q.selected = [label];
        q.other = '';
      }
      return { ...prev, [qIdx]: q };
    });
  }

  function selectOther(qIdx, multiSelect) {
    setAnswers(prev => {
      const q = { ...prev[qIdx] };
      if (!multiSelect) {
        q.selected = [];
      }
      return { ...prev, [qIdx]: q };
    });
  }

  return (
    <div className="flex justify-start mb-4">
      <div className="max-w-[85%] w-full space-y-4">
        {questions.map((q, qIdx) => {
          const a = answers[qIdx];
          const isOtherActive = a.selected.length === 0 && a.other !== '';
          return (
            <div key={qIdx} className="bg-gray-800/50 border border-gray-700 rounded-xl p-4">
              <div className="flex items-center gap-2 mb-3">
                {q.header && (
                  <span className="text-xs bg-gray-700 text-gray-300 rounded px-2 py-0.5 font-medium">
                    {q.header}
                  </span>
                )}
              </div>
              <p className="text-sm text-gray-200 font-medium mb-3">{q.question}</p>
              <div className="space-y-1.5">
                {(q.options || []).map((opt, oIdx) => {
                  const isSelected = a.selected.includes(opt.label);
                  return (
                    <button
                      key={oIdx}
                      onClick={() => toggleOption(qIdx, opt.label, q.multiSelect)}
                      className={`w-full flex items-start gap-3 p-2.5 rounded-lg text-left transition-colors ${
                        isSelected
                          ? 'bg-indigo-600/15 border border-indigo-500/30'
                          : 'border border-transparent hover:bg-gray-800'
                      }`}
                    >
                      <span className={`mt-0.5 shrink-0 w-4 h-4 rounded-${q.multiSelect ? 'sm' : 'full'} border-2 flex items-center justify-center ${
                        isSelected
                          ? 'border-indigo-500 bg-indigo-500'
                          : 'border-gray-600'
                      }`}>
                        {isSelected && (
                          q.multiSelect
                            ? <svg className="w-2.5 h-2.5 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={3}><path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" /></svg>
                            : <span className="w-1.5 h-1.5 bg-white rounded-full" />
                        )}
                      </span>
                      <div className="min-w-0">
                        <span className="text-sm text-gray-200">{opt.label}</span>
                        {opt.description && (
                          <p className="text-xs text-gray-500 mt-0.5">{opt.description}</p>
                        )}
                      </div>
                    </button>
                  );
                })}
                {/* "Other" option */}
                <div
                  className={`flex items-start gap-3 p-2.5 rounded-lg transition-colors ${
                    isOtherActive
                      ? 'bg-indigo-600/15 border border-indigo-500/30'
                      : 'border border-transparent hover:bg-gray-800'
                  }`}
                >
                  <button
                    onClick={() => selectOther(qIdx, q.multiSelect)}
                    className="mt-0.5 shrink-0"
                  >
                    <span className={`w-4 h-4 rounded-${q.multiSelect ? 'sm' : 'full'} border-2 flex items-center justify-center ${
                      isOtherActive
                        ? 'border-indigo-500 bg-indigo-500'
                        : 'border-gray-600'
                    }`}>
                      {isOtherActive && (
                        q.multiSelect
                          ? <svg className="w-2.5 h-2.5 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={3}><path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" /></svg>
                          : <span className="w-1.5 h-1.5 bg-white rounded-full" />
                      )}
                    </span>
                  </button>
                  <div className="flex-1 min-w-0">
                    <span className="text-sm text-gray-400">Other:</span>
                    <input
                      type="text"
                      value={a.other}
                      onChange={e => {
                        if (!q.multiSelect) {
                          setAnswers(prev => ({
                            ...prev,
                            [qIdx]: { ...prev[qIdx], selected: [], other: e.target.value },
                          }));
                        } else {
                          setAnswers(prev => ({
                            ...prev,
                            [qIdx]: { ...prev[qIdx], other: e.target.value },
                          }));
                        }
                      }}
                      onFocus={() => selectOther(qIdx, q.multiSelect)}
                      placeholder="Tell Claude what to do instead..."
                      className="mt-1 w-full bg-gray-900 border border-gray-700 rounded-lg px-3 py-1.5 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500/50"
                    />
                  </div>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// --- PlanComplete message with action badges ---

function PlanCompleteMessage({ onSend, pendingAnswersRef }) {
  const [action, setAction] = useState(null);

  function compileAnswersText() {
    const allAnswerSets = pendingAnswersRef?.current || {};
    const allEntries = Object.values(allAnswerSets).flatMap(a => Object.values(a));
    if (allEntries.length === 0) return '';
    const hasAnyAnswer = allEntries.some(a => a.selected?.length > 0 || a.other);
    if (!hasAnyAnswer) return '';
    let text = '\n\nAnswers to your questions:\n';
    for (const a of allEntries) {
      const answer = a.selected?.length > 0 ? a.selected.join(', ') : (a.other || '(no answer)');
      text += `- ${a.question} -> ${answer}\n`;
    }
    return text;
  }

  function handleExecute() {
    setAction('execute');
    const answers = compileAnswersText();
    onSend && onSend('Proceed with the plan. Execute all proposed changes.' + answers, 'default');
  }

  function handleRefine() {
    setAction('refine');
    const answers = compileAnswersText();
    onSend && onSend('Please revise the plan with my additional feedback.' + answers, 'plan');
  }

  if (action) {
    return (
      <div className="flex justify-center my-4">
        <div className={`flex items-center gap-2 rounded-xl px-4 py-2 text-sm font-medium ${
          action === 'execute'
            ? 'bg-indigo-600/20 border border-indigo-500/30 text-indigo-300'
            : 'bg-amber-950/30 border border-amber-500/30 text-amber-300'
        }`}>
          {action === 'execute' ? (
            <>
              <svg className="w-3.5 h-3.5" fill="currentColor" viewBox="0 0 24 24"><path d="M8 5v14l11-7z"/></svg>
              Executing plan
            </>
          ) : (
            <>
              <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}><path strokeLinecap="round" strokeLinejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" /></svg>
              Refining plan
            </>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="flex justify-center my-6">
      <div className="flex items-center gap-3 bg-amber-950/30 border border-amber-500/30 rounded-2xl px-5 py-3">
        <svg className="w-4 h-4 text-amber-400 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <span className="text-sm text-amber-300 font-medium">Plan complete</span>
        <button
          onClick={handleExecute}
          className="px-4 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium rounded-xl transition-colors"
        >
          Execute
        </button>
        <button
          onClick={handleRefine}
          className="px-4 py-1.5 bg-gray-700 hover:bg-gray-600 text-gray-300 text-sm font-medium rounded-xl transition-colors"
        >
          Refine
        </button>
      </div>
    </div>
  );
}

// --- Main renderer ---

export default function MessageRenderer({ message, onSend, pendingAnswersRef }) {
  switch (message.type) {
    case 'human':
      return (
        <div className="flex justify-end mb-4">
          <div className="max-w-[85%] bg-indigo-600/20 border border-indigo-500/30 rounded-2xl px-4 py-3">
            {message.images && message.images.length > 0 && (
              <div className="flex flex-wrap gap-2 mb-2">
                {message.images.map((img, i) => (
                  <img
                    key={i}
                    src={`data:${img.mediaType || 'image/png'};base64,${img.data}`}
                    alt="Attached"
                    className="max-h-48 rounded-lg border border-indigo-500/20 cursor-pointer hover:border-indigo-400/40 transition-colors"
                    onClick={(e) => {
                      const w = window.open();
                      if (w) {
                        w.document.write(`<img src="${e.target.src}" style="max-width:100%;background:#111">`);
                        w.document.title = 'Image';
                      }
                    }}
                  />
                ))}
              </div>
            )}
            <p className="text-gray-200 text-sm leading-relaxed whitespace-pre-wrap">{message.content}</p>
          </div>
        </div>
      );

    case 'assistant':
      return (
        <div className="flex justify-start mb-4">
          <div className="max-w-[85%]">
            <div className="text-gray-300 text-sm leading-relaxed prose-studio">
              {renderMarkdown(message.content)}
            </div>
          </div>
        </div>
      );

    case 'tool_use':
      if (message.hidden) return null;
      return <ToolUseMessage message={message} />;

    case 'tool_result':
      if (message.hidden) return null;
      return <ToolResultMessage message={message} />;

    case 'ask_user_question':
      return <AskUserQuestionWidget message={message} pendingAnswersRef={pendingAnswersRef} />;

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
      return <PlanCompleteMessage onSend={onSend} pendingAnswersRef={pendingAnswersRef} />;

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
