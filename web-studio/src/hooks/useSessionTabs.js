import { useState, useRef, useEffect, useCallback } from 'react';
import { updateMessagesFromStream } from '../utils/streamParser';
import { parseSessionMessages, extractLastTodos } from './useStudioWebSocket';

/**
 * Multi-session tab manager.
 * Consumes ws (pure transport from useStudioWebSocket) and manages
 * all per-session state: messages, todos, isStreaming, tabs, caching.
 */
export default function useSessionTabs(ws) {
  // --- Tabs state ---
  const [tabs, setTabs] = useState(() => {
    const saved = localStorage.getItem('studio-tabs');
    if (saved) {
      try { return JSON.parse(saved); } catch {}
    }
    // Migration: old single-session format
    const oldSid = localStorage.getItem('studio-session-id');
    if (oldSid) {
      localStorage.removeItem('studio-session-id');
      return [{ id: oldSid, label: 'Chat' }];
    }
    return [{ id: null, label: 'New Chat' }];
  });

  const [activeTabIndex, setActiveTabIndex] = useState(() =>
    parseInt(localStorage.getItem('studio-active-tab-index') || '0', 10)
  );

  // Ref-based session cache for perf — no re-render on background stream events
  // Map<sessionId|null, { messages: [], todos: [], isStreaming: false, hasUnread: false, lastMode: 'default', aborted: false, pendingAnswers: {} }>
  const sessionCacheRef = useRef(new Map());

  // Version counter to force re-render when the active session's data changes
  const [activeVersion, setActiveVersion] = useState(0);

  // --- Persistence ---
  useEffect(() => {
    localStorage.setItem('studio-tabs', JSON.stringify(tabs));
  }, [tabs]);

  useEffect(() => {
    localStorage.setItem('studio-active-tab-index', String(activeTabIndex));
  }, [activeTabIndex]);

  // --- Helpers ---
  const getActiveTab = useCallback(() => {
    return tabs[activeTabIndex] || tabs[0] || { id: null, label: 'New Chat' };
  }, [tabs, activeTabIndex]);

  const getCache = useCallback((sessionId) => {
    const key = sessionId || '__null__';
    if (!sessionCacheRef.current.has(key)) {
      sessionCacheRef.current.set(key, {
        messages: [],
        todos: [],
        isStreaming: false,
        hasUnread: false,
        lastMode: 'default',
        aborted: false,
        pendingAnswers: {},
        draft: { text: '', images: [] },
      });
    }
    return sessionCacheRef.current.get(key);
  }, []);

  const isActiveSession = useCallback((sessionId) => {
    const activeTab = tabs[activeTabIndex];
    if (!activeTab) return false;
    if (!sessionId && !activeTab.id) return true;
    return activeTab.id === sessionId;
  }, [tabs, activeTabIndex]);

  // --- Load session on tab switch if not cached ---
  const loadIfNeeded = useCallback((sessionId) => {
    if (!sessionId) return;
    const cache = getCache(sessionId);
    if (cache.messages.length === 0) {
      ws.loadSession(sessionId);
    }
  }, [ws, getCache]);

  // Load initial active tab session once WS connects (not on mount — WS not ready yet)
  useEffect(() => {
    if (!ws.connected) return;
    const activeTab = tabs[activeTabIndex];
    if (activeTab?.id) {
      loadIfNeeded(activeTab.id);
    }
  }, [ws.connected]); // eslint-disable-line react-hooks/exhaustive-deps

  // --- Event routing via ws.subscribe ---
  useEffect(() => {
    const unsubs = [];

    // stream events
    unsubs.push(ws.subscribe('stream', (data) => {
      const event = data.data || data.event || data;
      const sessionId = data.session_id || event.session_id || null;

      // If session_id comes in and matches no tab, assign to the active null tab
      let resolvedSessionId = sessionId;
      if (sessionId) {
        const tabIdx = tabs.findIndex(t => t.id === sessionId);
        if (tabIdx === -1) {
          // Find the active tab with id: null and assign this session_id
          const activeTab = tabs[activeTabIndex];
          if (activeTab && activeTab.id === null) {
            setTabs(prev => {
              const updated = [...prev];
              updated[activeTabIndex] = { ...updated[activeTabIndex], id: sessionId };
              return updated;
            });
            // Migrate cache from null to sessionId
            const nullKey = '__null__';
            if (sessionCacheRef.current.has(nullKey)) {
              const oldCache = sessionCacheRef.current.get(nullKey);
              sessionCacheRef.current.set(sessionId, oldCache);
              sessionCacheRef.current.delete(nullKey);
            }
          }
        }
      }

      const cache = getCache(resolvedSessionId);
      cache.messages = updateMessagesFromStream(cache.messages, event);
      cache.isStreaming = true;

      // Extract todos from TodoWrite tool_use events
      if (event.type === 'assistant') {
        const blocks = event.message?.content;
        if (Array.isArray(blocks)) {
          for (const block of blocks) {
            if (block.type === 'tool_use' && block.name === 'TodoWrite') {
              cache.todos = block.input?.todos || [];
            }
          }
        }
      }

      if (isActiveSession(resolvedSessionId)) {
        setActiveVersion(v => v + 1);
      } else {
        cache.hasUnread = true;
      }
    }));

    // done events
    unsubs.push(ws.subscribe('done', (data) => {
      const sessionId = data.session_id || null;
      const cache = getCache(sessionId);
      cache.isStreaming = false;

      // Add plan_complete marker if was in plan mode and not aborted
      if (cache.lastMode === 'plan' && !cache.aborted) {
        cache.messages = [...cache.messages, { type: 'plan_complete' }];
      }
      cache.aborted = false;

      // Refresh session list
      ws.sendRaw({ type: 'list_sessions' });

      // Update tab label from session list if still generic
      if (sessionId) {
        const tab = tabs.find(t => t.id === sessionId);
        if (tab && (tab.label === 'New Chat' || tab.label === 'Chat')) {
          // Label will be updated when sessions list refreshes
        }
      }

      if (isActiveSession(sessionId)) {
        setActiveVersion(v => v + 1);
      }
    }));

    // error events
    unsubs.push(ws.subscribe('error', (data) => {
      const sessionId = data.session_id || null;
      const cache = getCache(sessionId);
      cache.messages = [...cache.messages, { type: 'error', content: data.message || data.error || 'Unknown error' }];
      cache.isStreaming = false;

      if (isActiveSession(sessionId)) {
        setActiveVersion(v => v + 1);
      }
    }));

    // session_messages — loaded session history
    unsubs.push(ws.subscribe('session_messages', (data) => {
      const sessionId = data.session_id || null;
      const cache = getCache(sessionId);
      cache.messages = parseSessionMessages(data.messages || []);
      cache.todos = extractLastTodos(data.messages || []);

      if (isActiveSession(sessionId)) {
        setActiveVersion(v => v + 1);
      }
    }));

    // busy — session already running
    unsubs.push(ws.subscribe('busy', (data) => {
      const sessionId = data.session_id || null;
      const cache = getCache(sessionId);
      cache.messages = [...cache.messages, { type: 'error', content: 'Session is busy. Please wait for the current operation to complete.' }];

      if (isActiveSession(sessionId)) {
        setActiveVersion(v => v + 1);
      }
    }));

    // active_streams — which sessions are currently streaming
    unsubs.push(ws.subscribe('active_streams', (data) => {
      const activeIds = data.session_ids || [];
      // Mark sessions as streaming
      for (const sid of activeIds) {
        const cache = getCache(sid);
        cache.isStreaming = true;
      }
      setActiveVersion(v => v + 1);
    }));

    return () => unsubs.forEach(fn => fn());
  }, [ws, tabs, activeTabIndex, getCache, isActiveSession]);

  // --- Update tab labels from sessions list ---
  useEffect(() => {
    if (!ws.sessions || ws.sessions.length === 0) return;
    setTabs(prev => {
      let changed = false;
      const updated = prev.map(tab => {
        if (!tab.id) return tab;
        const session = ws.sessions.find(s => (s.session_id || s.id) === tab.id);
        if (session?.summary && tab.label !== session.summary) {
          changed = true;
          return { ...tab, label: session.summary };
        }
        return tab;
      });
      return changed ? updated : prev;
    });
  }, [ws.sessions]);

  // --- Public methods ---

  const newTab = useCallback(() => {
    setTabs(prev => [...prev, { id: null, label: 'New Chat' }]);
    setActiveTabIndex(tabs.length); // will be the new last index
  }, [tabs.length]);

  const openTab = useCallback((sessionId, label) => {
    // If already open, just focus
    const existingIdx = tabs.findIndex(t => t.id === sessionId);
    if (existingIdx !== -1) {
      setActiveTabIndex(existingIdx);
      const cache = getCache(sessionId);
      cache.hasUnread = false;
      loadIfNeeded(sessionId);
      setActiveVersion(v => v + 1);
      return;
    }
    // Otherwise push new tab and focus
    const newLabel = label || (sessionId ? sessionId.slice(0, 8) + '...' : 'New Chat');
    setTabs(prev => [...prev, { id: sessionId, label: newLabel }]);
    setActiveTabIndex(tabs.length);
    loadIfNeeded(sessionId);
  }, [tabs, getCache, loadIfNeeded]);

  const closeTab = useCallback((index) => {
    const tab = tabs[index];
    if (!tab) return;

    // If streaming, abort first
    if (tab.id) {
      const cache = getCache(tab.id);
      if (cache.isStreaming) {
        ws.abort(tab.id);
      }
    }

    setTabs(prev => {
      const updated = prev.filter((_, i) => i !== index);
      if (updated.length === 0) {
        return [{ id: null, label: 'New Chat' }];
      }
      return updated;
    });

    // Adjust active tab index
    if (index === activeTabIndex) {
      // Switch to the neighbor
      setActiveTabIndex(prev => {
        const newLength = tabs.length - 1;
        if (newLength <= 0) return 0;
        return Math.min(prev, newLength - 1);
      });
    } else if (index < activeTabIndex) {
      setActiveTabIndex(prev => prev - 1);
    }

    setActiveVersion(v => v + 1);
  }, [tabs, activeTabIndex, getCache, ws]);

  const switchTab = useCallback((index) => {
    if (index < 0 || index >= tabs.length) return;
    setActiveTabIndex(index);
    const tab = tabs[index];
    if (tab?.id) {
      const cache = getCache(tab.id);
      cache.hasUnread = false;
      loadIfNeeded(tab.id);
    }
    setActiveVersion(v => v + 1);
  }, [tabs, getCache, loadIfNeeded]);

  const sendPrompt = useCallback((text, mode, model, images) => {
    const activeTab = tabs[activeTabIndex];
    if (!activeTab) return;
    const sessionId = activeTab.id;

    // Fix 2: update label immediately from prompt text
    if (activeTab.label === 'New Chat' || activeTab.label === 'Chat') {
      const firstLine = text.split('\n').find(l => l.trim()) || text;
      const label = firstLine.slice(0, 50) + (firstLine.length > 50 ? '...' : '');
      setTabs(prev => {
        const updated = [...prev];
        updated[activeTabIndex] = { ...updated[activeTabIndex], label };
        return updated;
      });
    }

    const cache = getCache(sessionId);
    cache.lastMode = mode || 'default';
    cache.aborted = false;

    // Add human message to cache
    const humanMsg = { type: 'human', content: text };
    if (images && images.length > 0) {
      humanMsg.images = images;
    }
    cache.messages = [...cache.messages, humanMsg];
    cache.isStreaming = true;

    // Send via transport
    ws.sendPrompt(text, mode, model, images, sessionId);

    setActiveVersion(v => v + 1);
  }, [tabs, activeTabIndex, getCache, ws]);

  const abortActiveSession = useCallback(() => {
    const activeTab = tabs[activeTabIndex];
    if (!activeTab) return;
    const cache = getCache(activeTab.id);
    cache.aborted = true;
    cache.isStreaming = false;
    ws.abort(activeTab.id);
    setActiveVersion(v => v + 1);
  }, [tabs, activeTabIndex, getCache, ws]);

  const updateDraft = useCallback((text, images) => {
    const activeTab = tabs[activeTabIndex];
    if (!activeTab) return;
    const cache = getCache(activeTab.id);
    cache.draft = { text, images };
  }, [tabs, activeTabIndex, getCache]);

  const getActiveState = useCallback(() => {
    const activeTab = tabs[activeTabIndex];
    if (!activeTab) return { messages: [], todos: [], isStreaming: false };
    const cache = getCache(activeTab.id);
    return {
      messages: cache.messages,
      todos: cache.todos,
      isStreaming: cache.isStreaming,
      pendingAnswers: cache.pendingAnswers,
      draft: cache.draft,
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tabs, activeTabIndex, getCache, activeVersion]);

  const activeSessionId = tabs[activeTabIndex]?.id || null;

  // Compute active stream count
  const activeStreamCount = (() => {
    let count = 0;
    for (const [, cache] of sessionCacheRef.current) {
      if (cache.isStreaming) count++;
    }
    return count;
  })();

  // Enriched tabs with indicators
  const enrichedTabs = tabs.map(tab => {
    const cache = tab.id ? sessionCacheRef.current.get(tab.id) : sessionCacheRef.current.get('__null__');
    return {
      ...tab,
      isStreaming: cache?.isStreaming || false,
      hasUnread: cache?.hasUnread || false,
    };
  });

  // PendingAnswersRef for the active session (used by ChatPanel/MessageRenderer)
  const pendingAnswersRef = useRef({});
  // Sync pendingAnswersRef with active session cache
  useEffect(() => {
    const activeTab = tabs[activeTabIndex];
    if (activeTab) {
      const cache = getCache(activeTab.id);
      pendingAnswersRef.current = cache.pendingAnswers;
    }
  }, [tabs, activeTabIndex, getCache, activeVersion]);

  return {
    tabs: enrichedTabs,
    activeTabIndex,
    activeSessionId,
    activeStreamCount,
    pendingAnswersRef,
    newTab,
    openTab,
    closeTab,
    switchTab,
    sendPrompt,
    abortActiveSession,
    getActiveState,
    updateDraft,
  };
}
