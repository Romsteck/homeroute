import { useState, useRef, useEffect, useCallback } from 'react';

export default function InputBar({ onSend, onAbort, isStreaming, disabled }) {
  const [text, setText] = useState('');
  const textareaRef = useRef(null);

  useEffect(() => {
    if (!isStreaming && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [isStreaming]);

  const handleSubmit = useCallback(() => {
    const trimmed = text.trim();
    if (!trimmed || isStreaming || disabled) return;
    onSend(trimmed);
    setText('');
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  }, [text, isStreaming, disabled, onSend]);

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

  return (
    <div className="border-t border-gray-800 bg-gray-900/80 backdrop-blur px-4 py-3 shrink-0">
      <div className="max-w-[800px] mx-auto flex gap-2">
        <textarea
          ref={textareaRef}
          value={text}
          onChange={handleInput}
          onKeyDown={handleKeyDown}
          placeholder={disabled ? 'Disconnected...' : 'Message Claude...'}
          disabled={disabled || isStreaming}
          rows={1}
          className="flex-1 bg-gray-800/50 border border-gray-700 rounded-xl px-4 py-2.5 text-sm
                     text-gray-200 placeholder-gray-600 resize-none
                     focus:outline-none focus:ring-1 focus:ring-indigo-500/50 focus:border-indigo-500/50
                     disabled:opacity-50 transition-shadow"
          style={{ minHeight: '42px' }}
        />
        {isStreaming ? (
          <button
            onClick={onAbort}
            className="self-end px-4 py-2.5 bg-red-600 hover:bg-red-500 text-white
                       rounded-xl text-sm font-medium shrink-0 transition-colors"
          >
            Stop
          </button>
        ) : (
          <button
            onClick={handleSubmit}
            disabled={disabled || !text.trim()}
            className="self-end px-4 py-2.5 bg-indigo-600 hover:bg-indigo-500 text-white
                       rounded-xl text-sm font-medium disabled:opacity-30 disabled:cursor-not-allowed
                       shrink-0 transition-colors"
          >
            Send
          </button>
        )}
      </div>
    </div>
  );
}
