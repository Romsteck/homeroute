import MessageList from './MessageList';
import InputBar from './InputBar';

export default function ChatPanel({ messages, isStreaming, onSend, onAbort, connected }) {
  return (
    <div className="flex-1 flex flex-col min-h-0 min-w-0 bg-gray-900">
      <MessageList messages={messages} isStreaming={isStreaming} />
      <InputBar
        onSend={onSend}
        onAbort={onAbort}
        isStreaming={isStreaming}
        disabled={!connected}
      />
    </div>
  );
}
