import { useEffect, useRef, useCallback } from 'react';
import MessageRenderer from './MessageRenderer';

export default function MessageList({ messages, isStreaming, onSend, mode }) {
  const containerRef = useRef(null);
  const bottomRef = useRef(null);
  const shouldAutoScroll = useRef(true);

  const handleScroll = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const threshold = 80;
    shouldAutoScroll.current = el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
  }, []);

  useEffect(() => {
    if (shouldAutoScroll.current) {
      bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages, isStreaming]);

  return (
    <div
      ref={containerRef}
      onScroll={handleScroll}
      className="flex-1 overflow-y-auto py-6"
    >
      <div className="max-w-[800px] mx-auto px-6">
        {messages.length === 0 && (
          <div className="flex flex-col items-center justify-center h-full text-gray-600 pt-32">
            <svg className="w-12 h-12 mb-4 text-gray-800" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1} d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
            </svg>
            <span className="text-sm">Send a message to start a session.</span>
          </div>
        )}
        {messages.map((msg, i) => (
          <MessageRenderer key={i} message={msg} onSend={onSend} />
        ))}
        {isStreaming && (
          <div className="flex items-center gap-2 text-gray-500 text-xs ml-2 mb-4">
            <div className="flex gap-1">
              <span className={`w-1.5 h-1.5 ${mode === 'plan' ? 'bg-amber-500' : 'bg-purple-500'} rounded-full animate-bounce-dot`} style={{animationDelay: '0ms'}} />
              <span className={`w-1.5 h-1.5 ${mode === 'plan' ? 'bg-amber-500' : 'bg-purple-500'} rounded-full animate-bounce-dot`} style={{animationDelay: '150ms'}} />
              <span className={`w-1.5 h-1.5 ${mode === 'plan' ? 'bg-amber-500' : 'bg-purple-500'} rounded-full animate-bounce-dot`} style={{animationDelay: '300ms'}} />
            </div>
            <span>{mode === 'plan' ? 'Claude is analyzing...' : 'Claude is thinking...'}</span>
          </div>
        )}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
