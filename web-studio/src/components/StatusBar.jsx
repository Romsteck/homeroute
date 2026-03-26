export default function StatusBar({ connected, sessionId, isStreaming, activeCount }) {
  return (
    <div className="h-7 bg-gray-800 border-t border-gray-700 px-4 flex items-center justify-between text-xs shrink-0 font-mono">
      <div className="flex items-center gap-2">
        <span className={`inline-block w-1.5 h-1.5 rounded-full ${connected ? 'bg-green-500' : 'bg-red-500'}`}></span>
        <span className={connected ? 'text-gray-400' : 'text-red-400'}>
          {connected ? 'Connected' : 'Disconnected'}
        </span>
      </div>
      <div className="text-gray-500">
        {sessionId ? `Session: ${sessionId.slice(0, 8)}` : 'No session'}
      </div>
      <div className="flex items-center gap-2">
        {activeCount > 0 && (
          <>
            <span className="inline-block w-1.5 h-1.5 bg-purple-500 rounded-full animate-pulse"></span>
            <span className="text-purple-400">{activeCount} active</span>
          </>
        )}
        {isStreaming && !activeCount && (
          <>
            <span className="inline-block w-1.5 h-1.5 bg-amber-500 rounded-full animate-pulse"></span>
            <span className="text-amber-500">Streaming</span>
          </>
        )}
      </div>
    </div>
  );
}
