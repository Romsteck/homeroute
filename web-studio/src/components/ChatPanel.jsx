import { useState, useCallback, useRef } from 'react';
import MessageList from './MessageList';
import InputBar from './InputBar';
import TodoPanel from './TodoPanel';

export default function ChatPanel({ messages, isStreaming, onSend, onAbort, connected, todos, authStatus, onOpenAuthDialog }) {
  const [mode, setMode] = useState(() => localStorage.getItem('studio-mode') || 'default');
  const pendingAnswersRef = useRef({});

  const handleModeChange = useCallback((newMode) => {
    setMode(newMode);
    localStorage.setItem('studio-mode', newMode);
  }, []);

  // Wrap onSend from MessageRenderer to also switch mode when executing a plan
  const handleSendFromMessage = useCallback((text, sendMode, sendModel, images) => {
    if (sendMode === 'default' && mode === 'plan') {
      handleModeChange('default');
    }
    onSend(text, sendMode, sendModel, images);
  }, [onSend, mode, handleModeChange]);

  return (
    <div className="flex-1 flex flex-col min-h-0 min-w-0 bg-gray-900">
      <TodoPanel todos={todos} />
      <MessageList messages={messages} isStreaming={isStreaming} onSend={handleSendFromMessage} mode={mode} pendingAnswersRef={pendingAnswersRef} />
      <InputBar
        onSend={onSend}
        onAbort={onAbort}
        isStreaming={isStreaming}
        disabled={!connected}
        mode={mode}
        onModeChange={handleModeChange}
        authStatus={authStatus}
        onOpenAuthDialog={onOpenAuthDialog}
      />
    </div>
  );
}
