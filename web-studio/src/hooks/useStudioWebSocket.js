import { useState, useEffect, useRef, useCallback } from 'react';
import { updateMessagesFromStream } from '../utils/streamParser';

/**
 * Convert Claude JSONL session messages into display format.
 * JSONL format: {"type":"user|assistant","message":{"role":"...","content":[{"type":"text","text":"..."}]}}
 */
function parseSessionMessages(rawMessages) {
  const result = [];
  for (const msg of rawMessages) {
    const type = msg.type;
    if (type === 'user') {
      const content = extractTextContent(msg);
      if (content) {
        result.push({ type: 'human', content });
      }
    } else if (type === 'assistant') {
      const content = extractTextContent(msg);
      // Check for tool_use blocks in content array
      const contentArr = msg.message?.content;
      if (Array.isArray(contentArr)) {
        for (const block of contentArr) {
          if (block.type === 'text' && block.text) {
            result.push({ type: 'assistant', subtype: 'text', content: block.text, complete: true });
          } else if (block.type === 'tool_use') {
            result.push({ type: 'tool_use', tool: block.name, input: block.input });
          }
        }
      } else if (content) {
        result.push({ type: 'assistant', subtype: 'text', content, complete: true });
      }
    } else if (type === 'tool_result' || msg.message?.role === 'tool') {
      const contentArr = msg.message?.content;
      let text = '';
      let isError = false;
      if (Array.isArray(contentArr)) {
        text = contentArr.map(b => b.text || '').join('\n');
      }
      if (msg.message?.is_error || msg.is_error) isError = true;
      if (text || isError) {
        result.push({ type: 'tool_result', content: text, is_error: isError });
      }
    }
    // Skip queue-operation, file-history-snapshot, etc.
  }
  return result;
}

function extractTextContent(msg) {
  const content = msg.message?.content;
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) {
    return content
      .filter(b => b.type === 'text')
      .map(b => b.text || '')
      .join('\n');
  }
  return '';
}

export default function useStudioWebSocket() {
  const [connected, setConnected] = useState(false);
  const [messages, setMessages] = useState([]);
  const [sessions, setSessions] = useState([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [currentSessionId, _setCurrentSessionId] = useState(() => {
    return localStorage.getItem('studio-session-id') || null;
  });
  const setCurrentSessionId = useCallback((id) => {
    _setCurrentSessionId(id);
    if (id) {
      localStorage.setItem('studio-session-id', id);
    } else {
      localStorage.removeItem('studio-session-id');
    }
  }, []);
  const wsRef = useRef(null);
  const reconnectTimer = useRef(null);

  const connect = useCallback(() => {
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${protocol}//${location.host}/ws`);
    wsRef.current = ws;

    ws.onopen = () => {
      setConnected(true);
      ws.send(JSON.stringify({ type: 'list_sessions' }));
      // Restore last session if any
      const savedSession = localStorage.getItem('studio-session-id');
      if (savedSession) {
        ws.send(JSON.stringify({ type: 'get_session', session_id: savedSession }));
      }
    };

    ws.onclose = () => {
      setConnected(false);
      wsRef.current = null;
      reconnectTimer.current = setTimeout(connect, 3000);
    };

    ws.onerror = () => {
      ws.close();
    };

    ws.onmessage = (evt) => {
      let data;
      try {
        data = JSON.parse(evt.data);
      } catch {
        return;
      }

      switch (data.type) {
        case 'stream': {
          const event = data.data || data.event || data;
          setMessages(prev => updateMessagesFromStream(prev, event));
          // Extract session_id from stream events (init or result)
          if (event.session_id) {
            setCurrentSessionId(event.session_id);
          }
          setIsStreaming(true);
          break;
        }
        case 'done':
          setIsStreaming(false);
          if (data.session_id) {
            setCurrentSessionId(data.session_id);
          }
          // Refresh session list to pick up new summaries
          if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN) {
            wsRef.current.send(JSON.stringify({ type: 'list_sessions' }));
          }
          break;
        case 'error':
          setMessages(prev => [...prev, { type: 'error', content: data.message || data.error || 'Unknown error' }]);
          setIsStreaming(false);
          break;
        case 'sessions':
          setSessions(data.sessions || []);
          break;
        case 'session_messages':
          setMessages(parseSessionMessages(data.messages || []));
          setIsStreaming(false);
          break;
        case 'busy':
          setMessages(prev => [...prev, { type: 'error', content: 'Session is busy. Please wait for the current operation to complete.' }]);
          break;
        default:
          break;
      }
    };
  }, []);

  useEffect(() => {
    connect();
    return () => {
      clearTimeout(reconnectTimer.current);
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, [connect]);

  const sendPrompt = useCallback((text, mode = 'default') => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    setMessages(prev => [...prev, { type: 'human', content: text }]);
    setIsStreaming(true);
    const payload = { type: 'prompt', prompt: text, mode };
    if (currentSessionId) {
      payload.session_id = currentSessionId;
    }
    wsRef.current.send(JSON.stringify(payload));
  }, [currentSessionId]);

  const abort = useCallback(() => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    wsRef.current.send(JSON.stringify({ type: 'abort' }));
  }, []);

  const loadSession = useCallback((sessionId) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    setCurrentSessionId(sessionId);
    wsRef.current.send(JSON.stringify({ type: 'get_session', session_id: sessionId }));
  }, []);

  const newSession = useCallback(() => {
    setMessages([]);
    setCurrentSessionId(null);
  }, []);

  const deleteSession = useCallback((sessionId) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    wsRef.current.send(JSON.stringify({ type: 'delete_session', session_id: sessionId }));
    // If deleting the current session, clear it
    if (sessionId === currentSessionId) {
      setMessages([]);
      setCurrentSessionId(null);
    }
  }, [currentSessionId]);

  return {
    connected,
    messages,
    sessions,
    isStreaming,
    currentSessionId,
    sendPrompt,
    abort,
    loadSession,
    newSession,
    deleteSession,
  };
}
