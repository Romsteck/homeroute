import { useState, useCallback } from 'react';
import MessageList from './MessageList';
import InputBar from './InputBar';

export default function ChatPanel({ messages, isStreaming, onSend, onAbort, connected }) {
  const [mode, setMode] = useState(() => localStorage.getItem('studio-mode') || 'default');

  const handleModeChange = useCallback((newMode) => {
    setMode(newMode);
    localStorage.setItem('studio-mode', newMode);
  }, []);

  // Wrap onSend from MessageRenderer to also switch mode when executing a plan
  const handleSendFromMessage = useCallback((text, sendMode, sendModel) => {
    if (sendMode === 'default' && mode === 'plan') {
      handleModeChange('default');
    }
    onSend(text, sendMode, sendModel);
  }, [onSend, mode, handleModeChange]);

  return (
    <div className="flex-1 flex flex-col min-h-0 min-w-0 bg-gray-900">
      <MessageList messages={messages} isStreaming={isStreaming} onSend={handleSendFromMessage} />
      <InputBar
        onSend={onSend}
        onAbort={onAbort}
        isStreaming={isStreaming}
        disabled={!connected}
        mode={mode}
        onModeChange={handleModeChange}
      />
    </div>
  );
}
