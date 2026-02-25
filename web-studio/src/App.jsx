import { useState, useEffect, useMemo, useCallback } from 'react';
import useStudioWebSocket from './hooks/useStudioWebSocket';
import useClaudeAuth from './hooks/useClaudeAuth';
import Header from './components/Header';
import ChatPanel from './components/ChatPanel';
import PreviewPanel from './components/PreviewPanel';
import FilesPanel from './components/FilesPanel';
import StatusBar from './components/StatusBar';
import AuthDialog from './components/AuthDialog';

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
  const [activeTab, setActiveTab] = useState(() => {
    return localStorage.getItem('studio-active-tab') || 'studio';
  });
  const ws = useStudioWebSocket();
  const auth = useClaudeAuth(ws.subscribe, ws.sendRaw, ws.connected);
  const appInfo = useMemo(() => getAppInfo(), []);

  useEffect(() => {
    document.title = `Studio - ${appInfo.appName}`;
  }, [appInfo.appName]);

  const handleTabChange = useCallback((tab) => {
    setActiveTab(tab);
    localStorage.setItem('studio-active-tab', tab);
  }, []);

  return (
    <div className="h-screen flex flex-col bg-gray-900">
      <Header
        appName={appInfo.appName}
        activeTab={activeTab}
        onTabChange={handleTabChange}
        connected={ws.connected}
        sessions={ws.sessions}
        currentSessionId={ws.currentSessionId}
        onSelectSession={ws.loadSession}
        onNewSession={ws.newSession}
        onDeleteSession={ws.deleteSession}
        authStatus={auth.authStatus}
        onOpenAuthDialog={auth.openAuthDialog}
      />

      <div className="flex flex-1 min-h-0">
        {/* Studio tab - always mounted, hidden when inactive */}
        <div className="flex flex-1 min-h-0" style={{display: activeTab === 'studio' ? 'flex' : 'none'}}>
          <div className="w-[30%] min-w-[300px] flex flex-col border-r border-gray-800">
            <ChatPanel
              messages={ws.messages}
              isStreaming={ws.isStreaming}
              onSend={ws.sendPrompt}
              onAbort={ws.abort}
              connected={ws.connected}
              todos={ws.todos}
              authStatus={auth.authStatus}
              onOpenAuthDialog={auth.openAuthDialog}
            />
          </div>
          <div className="flex-1 flex flex-col min-w-0">
            <PreviewPanel slug={appInfo.slug} domain={appInfo.domain} mode="split" sendRaw={ws.sendRaw} />
          </div>
        </div>
        {/* Preview tab - always mounted, hidden when inactive */}
        <div className="flex-1" style={{display: activeTab === 'preview' ? 'flex' : 'none'}}>
          <PreviewPanel slug={appInfo.slug} domain={appInfo.domain} mode="full" sendRaw={ws.sendRaw} />
        </div>
        {/* Files tab - only mounted when active */}
        {activeTab === 'files' && <FilesPanel sendRaw={ws.sendRaw} subscribe={ws.subscribe} connected={ws.connected} />}
      </div>

      <StatusBar
        connected={ws.connected}
        sessionId={ws.currentSessionId}
        isStreaming={ws.isStreaming}
      />

      {auth.showAuthDialog && (
        <AuthDialog
          authStatus={auth.authStatus}
          authEvent={auth.authEvent}
          onClose={auth.closeAuthDialog}
          sendRaw={ws.sendRaw}
          onUnlink={auth.unlinkAuth}
          setAuthEvent={auth.setAuthEvent}
        />
      )}
    </div>
  );
}
