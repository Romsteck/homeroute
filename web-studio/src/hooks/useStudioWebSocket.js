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
      const images = extractImageContent(msg);
      if (content || images.length > 0) {
        const humanMsg = { type: 'human', content: content || '' };
        if (images.length > 0) humanMsg.images = images;
        result.push(humanMsg);
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
            if (block.name === 'AskUserQuestion') {
              const questions = block.input?.questions || [];
              // Dedup: skip if previous message has identical questions
              const prev = result[result.length - 1];
              if (prev?.type === 'ask_user_question' &&
                  JSON.stringify(prev.questions) === JSON.stringify(questions)) {
                result[result.length - 1] = { ...prev, tool_use_id: block.id };
              } else {
                result.push({ type: 'ask_user_question', questions, tool_use_id: block.id });
              }
            } else if (block.name === 'TodoWrite') {
              result.push({ type: 'tool_use', tool: 'TodoWrite', input: block.input, tool_use_id: block.id, hidden: true });
            } else {
              result.push({ type: 'tool_use', tool: block.name, input: block.input });
            }
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
        // Annotate the last tool_use/ask_user_question with success/error status
        let annotatedType = null;
        let annotatedHidden = false;
        for (let j = result.length - 1; j >= 0; j--) {
          const m = result[j];
          if ((m.type === 'tool_use' || m.type === 'ask_user_question') && !m.status) {
            result[j] = { ...m, status: isError ? 'error' : 'success' };
            annotatedType = m.type;
            annotatedHidden = m.hidden || false;
            break;
          }
        }
        result.push({ type: 'tool_result', content: text, is_error: isError, hidden: annotatedType === 'ask_user_question' || annotatedHidden });
      }
    }
    // Skip queue-operation, file-history-snapshot, etc.
  }
  return result;
}

/**
 * Scan raw session messages (JSONL) backward for the last TodoWrite tool_use.
 * Returns the todos array or [].
 */
function extractLastTodos(rawMessages) {
  for (let i = rawMessages.length - 1; i >= 0; i--) {
    const content = rawMessages[i].message?.content;
    if (Array.isArray(content)) {
      for (let j = content.length - 1; j >= 0; j--) {
        if (content[j].type === 'tool_use' && content[j].name === 'TodoWrite') {
          return content[j].input?.todos || [];
        }
      }
    }
  }
  return [];
}

function extractImageContent(msg) {
  const content = msg.message?.content;
  if (!Array.isArray(content)) return [];
  return content
    .filter(b => b.type === 'image' && b.source?.type === 'base64')
    .map(b => ({ data: b.source.data, mediaType: b.source.media_type || 'image/png' }));
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
  const [todos, setTodos] = useState([]);
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
  const lastModeRef = useRef('default');
  const listenersRef = useRef({});
  const abortedRef = useRef(false);

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

      // Dispatch to external subscribers
      const listeners = listenersRef.current[data.type];
      if (listeners && listeners.length > 0) {
        for (const cb of listeners) {
          cb(data);
        }
        // For non-chat types, don't fall through to switch
        if (!['stream', 'done', 'error', 'sessions', 'session_messages', 'busy'].includes(data.type)) {
          return;
        }
      }

      switch (data.type) {
        case 'stream': {
          const event = data.data || data.event || data;
          setMessages(prev => updateMessagesFromStream(prev, event));
          // Extract session_id from stream events (init or result)
          if (event.session_id) {
            setCurrentSessionId(event.session_id);
          }
          // Extract todos from TodoWrite tool_use events
          if (event.type === 'assistant') {
            const blocks = event.message?.content;
            if (Array.isArray(blocks)) {
              for (const block of blocks) {
                if (block.type === 'tool_use' && block.name === 'TodoWrite') {
                  setTodos(block.input?.todos || []);
                }
              }
            }
          }
          setIsStreaming(true);
          break;
        }
        case 'done':
          setIsStreaming(false);
          if (data.session_id) {
            setCurrentSessionId(data.session_id);
          }
          // If we were in plan mode, add a plan_complete action message (skip if aborted)
          if (lastModeRef.current === 'plan' && !abortedRef.current) {
            setMessages(prev => [...prev, { type: 'plan_complete' }]);
          }
          abortedRef.current = false;
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
          setTodos(extractLastTodos(data.messages || []));
          // Don't force isStreaming=false here — if a stream is active,
          // replayed buffer events will follow and set isStreaming=true.
          // On fresh page load, isStreaming defaults to false already.
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

  const sendPrompt = useCallback((text, mode = 'default', model, images) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    abortedRef.current = false;
    lastModeRef.current = mode;
    // Build display message with optional images
    const humanMsg = { type: 'human', content: text };
    if (images && images.length > 0) {
      humanMsg.images = images;
    }
    setMessages(prev => [...prev, humanMsg]);
    setIsStreaming(true);
    const payload = { type: 'prompt', prompt: text, mode };
    if (currentSessionId) {
      payload.session_id = currentSessionId;
    }
    if (model) {
      payload.model = model;
    }
    if (images && images.length > 0) {
      payload.images = images;
    }
    wsRef.current.send(JSON.stringify(payload));
  }, [currentSessionId]);

  const abort = useCallback(() => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    abortedRef.current = true;
    setIsStreaming(false);
    wsRef.current.send(JSON.stringify({ type: 'abort' }));
  }, []);

  const loadSession = useCallback((sessionId) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    setCurrentSessionId(sessionId);
    wsRef.current.send(JSON.stringify({ type: 'get_session', session_id: sessionId }));
  }, []);

  const newSession = useCallback(() => {
    setMessages([]);
    setTodos([]);
    setCurrentSessionId(null);
  }, []);

  const subscribe = useCallback((type, callback) => {
    if (!listenersRef.current[type]) {
      listenersRef.current[type] = [];
    }
    listenersRef.current[type].push(callback);
    return () => {
      listenersRef.current[type] = listenersRef.current[type].filter(cb => cb !== callback);
    };
  }, []);

  const sendRaw = useCallback((msg) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    wsRef.current.send(JSON.stringify(msg));
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
    todos,
    sendPrompt,
    abort,
    loadSession,
    newSession,
    deleteSession,
    sendRaw,
    subscribe,
  };
}
