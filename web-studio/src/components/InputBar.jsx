import { useState, useRef, useEffect, useCallback } from 'react';

export default function InputBar({ onSend, onAbort, isStreaming, disabled }) {
  const [text, setText] = useState('');
  const [mode, setMode] = useState(() => localStorage.getItem('studio-mode') || 'default');
  const textareaRef = useRef(null);

  useEffect(() => {
    if (!isStreaming && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [isStreaming]);

  const handleSubmit = useCallback(() => {
    const trimmed = text.trim();
    if (!trimmed || isStreaming || disabled) return;
    onSend(trimmed, mode);
    setText('');
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  }, [text, isStreaming, disabled, onSend, mode]);

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
    setMode(next);
    localStorage.setItem('studio-mode', next);
  };

  const isPlan = mode === 'plan';

  return (
    <div className="border-t border-gray-800 bg-gray-900/80 backdrop-blur px-4 py-3 shrink-0">
      <div className="max-w-[800px] mx-auto flex gap-2 items-end">
        {/* Mode toggle */}
        <button
          onClick={toggleMode}
          disabled={isStreaming}
          title={isPlan ? 'Plan Mode: Claude analyzes without modifying files' : 'Execute Mode: Claude can read, write, and run commands'}
          className={`shrink-0 px-3 py-2 rounded-xl text-xs font-semibold transition-all border ${
            isPlan
              ? 'bg-amber-600/20 border-amber-500/40 text-amber-400 hover:bg-amber-600/30'
              : 'bg-gray-800/50 border-gray-700 text-gray-500 hover:text-gray-300 hover:border-gray-600'
          } disabled:opacity-50`}
        >
          {isPlan ? (
            <span className="flex items-center gap-1.5">
              <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z" />
                <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
              </svg>
              Plan
            </span>
          ) : (
            <span className="flex items-center gap-1.5">
              <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M5.25 5.653c0-.856.917-1.398 1.667-.986l11.54 6.348a1.125 1.125 0 010 1.971l-11.54 6.347a1.125 1.125 0 01-1.667-.985V5.653z" />
              </svg>
              Execute
            </span>
          )}
        </button>

        {/* Input */}
        <textarea
          ref={textareaRef}
          value={text}
          onChange={handleInput}
          onKeyDown={handleKeyDown}
          placeholder={disabled ? 'Disconnected...' : isPlan ? 'Ask Claude to plan...' : 'Message Claude...'}
          disabled={disabled || isStreaming}
          rows={1}
          className={`flex-1 bg-gray-800/50 border rounded-xl px-4 py-2.5 text-sm
                     text-gray-200 placeholder-gray-600 resize-none
                     focus:outline-none focus:ring-1 transition-shadow
                     disabled:opacity-50 ${
                       isPlan
                         ? 'border-amber-500/30 focus:ring-amber-500/50 focus:border-amber-500/50'
                         : 'border-gray-700 focus:ring-indigo-500/50 focus:border-indigo-500/50'
                     }`}
          style={{ minHeight: '42px' }}
        />

        {/* Send/Stop */}
        {isStreaming ? (
          <button
            onClick={onAbort}
            className="shrink-0 px-4 py-2.5 bg-red-600 hover:bg-red-500 text-white
                       rounded-xl text-sm font-medium transition-colors"
          >
            Stop
          </button>
        ) : (
          <button
            onClick={handleSubmit}
            disabled={disabled || !text.trim()}
            className={`shrink-0 px-4 py-2.5 text-white rounded-xl text-sm font-medium
                       disabled:opacity-30 disabled:cursor-not-allowed transition-colors ${
                         isPlan
                           ? 'bg-amber-600 hover:bg-amber-500'
                           : 'bg-indigo-600 hover:bg-indigo-500'
                       }`}
          >
            {isPlan ? 'Plan' : 'Send'}
          </button>
        )}
      </div>
    </div>
  );
}
