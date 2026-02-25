import { useState, useEffect, useRef, useCallback } from 'react';

/**
 * Convert Claude JSONL session messages into display format.
 * Exported for use by useSessionTabs.
 */
export function parseSessionMessages(rawMessages) {
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
      const contentArr = msg.message?.content;
      if (Array.isArray(contentArr)) {
        for (const block of contentArr) {
          if (block.type === 'text' && block.text) {
            result.push({ type: 'assistant', subtype: 'text', content: block.text, complete: true });
          } else if (block.type === 'tool_use') {
            if (block.name === 'AskUserQuestion') {
              const questions = block.input?.questions || [];
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
      } else {
        const content = extractTextContent(msg);
        if (content) {
          result.push({ type: 'assistant', subtype: 'text', content, complete: true });
        }
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
  }
  return result;
}

/**
 * Scan raw session messages (JSONL) backward for the last TodoWrite tool_use.
 * Returns the todos array or [].
 */
export function extractLastTodos(rawMessages) {
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

/**
 * Pure WebSocket transport hook.
 * Manages connection, sessions list, and subscriber dispatch.
 * No per-session state (messages, todos, isStreaming) — that lives in useSessionTabs.
 */
export default function useStudioWebSocket() {
  const [connected, setConnected] = useState(false);
  const [sessions, setSessions] = useState([]);
  const wsRef = useRef(null);
  const reconnectTimer = useRef(null);
  const listenersRef = useRef({});

  const connect = useCallback(() => {
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${protocol}//${location.host}/ws`);
    wsRef.current = ws;

    ws.onopen = () => {
      setConnected(true);
      ws.send(JSON.stringify({ type: 'list_sessions' }));
      ws.send(JSON.stringify({ type: 'get_active_streams' }));
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

      // Dispatch to all subscribers for this message type
      const listeners = listenersRef.current[data.type];
      if (listeners && listeners.length > 0) {
        for (const cb of listeners) {
          cb(data);
        }
      }

      // Only handle sessions globally — everything else is dispatched via subscribe
      switch (data.type) {
        case 'sessions':
          setSessions(data.sessions || []);
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

  const sendPrompt = useCallback((text, mode = 'default', model, images, sessionId) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    const payload = { type: 'prompt', prompt: text, mode };
    if (sessionId) {
      payload.session_id = sessionId;
    }
    if (model) {
      payload.model = model;
    }
    if (images && images.length > 0) {
      payload.images = images;
    }
    wsRef.current.send(JSON.stringify(payload));
  }, []);

  const abort = useCallback((sessionId) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    const payload = { type: 'abort' };
    if (sessionId) {
      payload.session_id = sessionId;
    }
    wsRef.current.send(JSON.stringify(payload));
  }, []);

  const loadSession = useCallback((sessionId) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    wsRef.current.send(JSON.stringify({ type: 'get_session', session_id: sessionId }));
  }, []);

  const deleteSession = useCallback((sessionId) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    wsRef.current.send(JSON.stringify({ type: 'delete_session', session_id: sessionId }));
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

  return {
    connected,
    sessions,
    sendPrompt,
    abort,
    loadSession,
    deleteSession,
    sendRaw,
    subscribe,
  };
}
