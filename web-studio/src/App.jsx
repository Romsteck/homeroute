import { useState, useEffect, useMemo, useCallback } from 'react';
import useStudioWebSocket from './hooks/useStudioWebSocket';
import Header from './components/Header';
import ChatPanel from './components/ChatPanel';
import PreviewPanel from './components/PreviewPanel';
import FilesPanel from './components/FilesPanel';
import ActivityPanel from './components/ActivityPanel';
import StatusBar from './components/StatusBar';

function getAppInfo() {
  const hostname = window.location.hostname;
  const parts = hostname.split('.');
  if (parts.length >= 3 && parts[0] === 'studio') {
    const slug = parts[1];
    const domain = parts.slice(2).join('.');
    return {
      slug,
      domain,
      appName: slug.charAt(0).toUpperCase() + slug.slice(1),
    };
  }
  return { slug: 'dev', domain: 'mynetwk.biz', appName: 'Dev' };
}

export default function App() {
  const [activeTab, setActiveTab] = useState('agent');
  const [activityPanelOpen, setActivityPanelOpen] = useState(true);
  const ws = useStudioWebSocket();
  const appInfo = useMemo(() => getAppInfo(), []);

  useEffect(() => {
    document.title = `Studio - ${appInfo.appName}`;
  }, [appInfo.appName]);

  const activities = useMemo(() => {
    const acts = [];
    for (let i = 0; i < ws.messages.length; i++) {
      const msg = ws.messages[i];
      if (msg.type === 'tool_use') {
        const activity = {
          id: i,
          tool: msg.tool,
          description: getToolDescription(msg),
          status: 'done',
          timestamp: new Date(),
        };
        const next = ws.messages[i + 1];
        if (next && next.type === 'tool_result' && next.is_error) {
          activity.status = 'error';
        }
        acts.push(activity);
      }
    }
    if (ws.isStreaming && acts.length > 0) {
      const lastAct = acts[acts.length - 1];
      const lastMsg = ws.messages[ws.messages.length - 1];
      if (lastMsg && lastMsg.type === 'tool_use') {
        lastAct.status = 'running';
      }
    }
    return acts;
  }, [ws.messages, ws.isStreaming]);

  const toggleActivity = useCallback(() => setActivityPanelOpen(s => !s), []);

  return (
    <div className="h-screen flex flex-col bg-gray-900">
      <Header
        appName={appInfo.appName}
        activeTab={activeTab}
        onTabChange={setActiveTab}
        connected={ws.connected}
        onToggleActivity={toggleActivity}
        activityPanelOpen={activityPanelOpen}
        activityCount={activities.length}
        sessions={ws.sessions}
        currentSessionId={ws.currentSessionId}
        onSelectSession={ws.loadSession}
        onNewSession={ws.newSession}
        onDeleteSession={ws.deleteSession}
      />

      <div className="flex flex-1 min-h-0">
        {/* Main panel area */}
        <div className="flex-1 flex flex-col min-h-0 min-w-0">
          {activeTab === 'agent' && (
            <ChatPanel
              messages={ws.messages}
              isStreaming={ws.isStreaming}
              onSend={ws.sendPrompt}
              onAbort={ws.abort}
              connected={ws.connected}
            />
          )}
          {activeTab === 'preview' && (
            <PreviewPanel slug={appInfo.slug} domain={appInfo.domain} />
          )}
          {activeTab === 'files' && <FilesPanel />}
        </div>

        {/* Activity side panel */}
        {activityPanelOpen && (
          <ActivityPanel
            activities={activities}
            onClose={() => setActivityPanelOpen(false)}
          />
        )}
      </div>

      <StatusBar
        connected={ws.connected}
        sessionId={ws.currentSessionId}
        isStreaming={ws.isStreaming}
      />
    </div>
  );
}

function getToolDescription(msg) {
  if (!msg.input) return '';
  const input = msg.input;
  if (input.file_path) return input.file_path.split('/').pop();
  if (input.command) {
    const cmd = input.command;
    return cmd.length > 60 ? cmd.slice(0, 60) + '...' : cmd;
  }
  if (input.pattern) return input.pattern;
  if (input.query) return input.query.length > 50 ? input.query.slice(0, 50) + '...' : input.query;
  return '';
}
