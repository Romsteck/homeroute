import { useState, useRef, useEffect, useCallback } from 'react';

const MODELS = [
  { value: 'claude-sonnet-4-6', label: 'Sonnet 4.6' },
  { value: 'claude-opus-4-6', label: 'Opus 4.6' },
  { value: 'claude-haiku-4-5-20251001', label: 'Haiku 4.5' },
];

export default function InputBar({ onSend, onAbort, isStreaming, disabled, mode, onModeChange }) {
  const [text, setText] = useState('');
  const [model, setModel] = useState(() => localStorage.getItem('studio-model') || 'claude-opus-4-6');
  const [modelOpen, setModelOpen] = useState(false);
  const textareaRef = useRef(null);
  const gearRef = useRef(null);

  useEffect(() => {
    if (!isStreaming && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [isStreaming]);

  // Close model picker on outside click
  useEffect(() => {
    if (!modelOpen) return;
    const handler = (e) => {
      if (gearRef.current && !gearRef.current.contains(e.target)) {
        setModelOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [modelOpen]);

  const handleSubmit = useCallback(() => {
    const trimmed = text.trim();
    if (!trimmed || isStreaming || disabled) return;
    onSend(trimmed, mode, model);
    setText('');
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  }, [text, isStreaming, disabled, onSend, mode, model]);

  const handleKeyDown = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  const handleInput = (e) => {
    setText(e.target.value);
    const el = e.target;
    el.style.height = 'auto';
    const lineHeight = 20;
    const maxHeight = lineHeight * 8;
    el.style.height = Math.min(el.scrollHeight, maxHeight) + 'px';
  };

  const toggleMode = () => {
    const next = mode === 'default' ? 'plan' : 'default';
    onModeChange(next);
  };

  const selectModel = (value) => {
    setModel(value);
    localStorage.setItem('studio-model', value);
    setModelOpen(false);
  };

  const currentModelLabel = MODELS.find(m => m.value === model)?.label || 'Opus 4.6';
  const isPlan = mode === 'plan';

  return (
    <div className="bg-gray-950 border-t border-gray-700 shrink-0">
      {/* Toolbar row */}
      <div className="flex items-center gap-2 px-4 py-1.5 text-xs text-gray-500">
        {/* Model selector - gear + label */}
        <div className="relative" ref={gearRef}>
          <button
            onClick={() => setModelOpen(!modelOpen)}
            disabled={isStreaming}
            title={`Model: ${currentModelLabel}`}
            className="flex items-center gap-1.5 px-1.5 py-1 rounded text-gray-500 hover:text-gray-300 hover:bg-gray-800/50 transition-colors disabled:opacity-50"
          >
            <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 011.37.49l1.296 2.247a1.125 1.125 0 01-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a7.723 7.723 0 010 .255c-.008.378.137.75.43.991l1.004.827c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 01-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.47 6.47 0 01-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.941-1.11.941h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 01-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 01-1.369-.49l-1.297-2.247a1.125 1.125 0 01.26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 010-.255c.007-.38-.138-.751-.43-.992l-1.004-.827a1.125 1.125 0 01-.26-1.43l1.297-2.247a1.125 1.125 0 011.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.086.22-.128.332-.183.582-.495.644-.869l.214-1.28z" />
              <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
            </svg>
            <span>{currentModelLabel}</span>
          </button>

          {modelOpen && (
            <div
              className="absolute bottom-full left-0 mb-2 bg-gray-900 border border-gray-700 rounded-lg shadow-xl shadow-black/40 py-1 min-w-[140px]"
              style={{ zIndex: 99999 }}
              onMouseDown={(e) => e.stopPropagation()}
            >
              {MODELS.map((m) => (
                <button
                  key={m.value}
                  onClick={() => selectModel(m.value)}
                  className={`w-full text-left px-3 py-1.5 text-xs transition-colors ${
                    model === m.value
                      ? 'text-indigo-400 bg-indigo-600/15'
                      : 'text-gray-400 hover:text-gray-200 hover:bg-gray-800'
                  }`}
                >
                  {m.label}
                </button>
              ))}
            </div>
          )}
        </div>

        <span className="text-gray-700">|</span>

        {/* Mode toggle */}
        <button
          onClick={toggleMode}
          disabled={isStreaming}
          title={isPlan ? 'Plan Mode: Claude analyzes without modifying files' : 'Execute Mode: Claude can read, write, and run commands'}
          className={`flex items-center gap-1.5 px-1.5 py-1 rounded transition-colors disabled:opacity-50 ${
            isPlan
              ? 'text-amber-400 hover:text-amber-300 hover:bg-amber-600/10'
              : 'text-gray-500 hover:text-gray-300 hover:bg-gray-800/50'
          }`}
        >
          {isPlan ? (
            <>
              <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z" />
                <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
              </svg>
              <span>Plan</span>
            </>
          ) : (
            <>
              <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M5.25 5.653c0-.856.917-1.398 1.667-.986l11.54 6.348a1.125 1.125 0 010 1.971l-11.54 6.347a1.125 1.125 0 01-1.667-.985V5.653z" />
              </svg>
              <span>Execute</span>
            </>
          )}
        </button>
      </div>

      {/* Textarea row */}
      <div className="relative">
        <textarea
          ref={textareaRef}
          value={text}
          onChange={handleInput}
          onKeyDown={handleKeyDown}
          placeholder={disabled ? 'Disconnected...' : isPlan ? 'Ask Claude to plan...' : 'Message Claude...'}
          disabled={disabled || isStreaming}
          rows={1}
          className="w-full bg-transparent border-0 focus:ring-0 focus:outline-none px-4 py-3 text-sm text-gray-200 placeholder-gray-600 resize-none disabled:opacity-50"
          style={{ minHeight: '42px' }}
        />

        {/* Send/Stop button - positioned inside textarea area */}
        <div className="absolute right-3 bottom-3">
          {isStreaming ? (
            <button
              onClick={onAbort}
              title="Stop"
              className="flex items-center justify-center w-7 h-7 bg-red-600 hover:bg-red-500 text-white rounded-md transition-colors"
            >
              <svg className="w-3 h-3" fill="currentColor" viewBox="0 0 24 24">
                <rect x="6" y="6" width="12" height="12" rx="1" />
              </svg>
            </button>
          ) : (
            <button
              onClick={handleSubmit}
              disabled={disabled || !text.trim()}
              title={isPlan ? 'Plan' : 'Send'}
              className={`flex items-center justify-center w-7 h-7 text-white rounded-md
                         disabled:opacity-30 disabled:cursor-not-allowed transition-colors ${
                           isPlan
                             ? 'bg-amber-600 hover:bg-amber-500'
                             : 'bg-indigo-600 hover:bg-indigo-500'
                         }`}
            >
              <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M4.5 10.5L12 3m0 0l7.5 7.5M12 3v18" />
              </svg>
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
