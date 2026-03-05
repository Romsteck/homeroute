import { useState, useEffect, useMemo, useCallback } from 'react';
import useStudioWebSocket from './hooks/useStudioWebSocket';
import useSessionTabs from './hooks/useSessionTabs';
import useClaudeAuth from './hooks/useClaudeAuth';
import Header from './components/Header';
import SessionTabs from './components/SessionTabs';
import ChatPanel from './components/ChatPanel';
import PreviewPanel from './components/PreviewPanel';
import FilesPanel from './components/FilesPanel';
import DocsPanel from './components/DocsPanel';
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
  const sessionManager = useSessionTabs(ws);
  const auth = useClaudeAuth(ws.subscribe, ws.sendRaw, ws.connected);
  const appInfo = useMemo(() => getAppInfo(), []);
  const activeState = sessionManager.getActiveState();

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
        currentSessionId={sessionManager.activeSessionId}
        onSelectSession={(id, label) => sessionManager.openTab(id, label)}
        onNewSession={sessionManager.newTab}
        onDeleteSession={ws.deleteSession}
        authStatus={auth.authStatus}
        onOpenAuthDialog={auth.openAuthDialog}
      />

      {/* Session tabs - only visible in studio mode */}
      {activeTab === 'studio' && (
        <SessionTabs
          tabs={sessionManager.tabs}
          activeIndex={sessionManager.activeTabIndex}
          onSwitch={sessionManager.switchTab}
          onClose={sessionManager.closeTab}
          onNew={sessionManager.newTab}
        />
      )}

      <div className="flex flex-1 min-h-0">
        {/* Studio tab - always mounted, hidden when inactive */}
        <div className="flex flex-1 min-h-0" style={{display: activeTab === 'studio' ? 'flex' : 'none'}}>
          <div className="w-[30%] min-w-[300px] flex flex-col border-r border-gray-800">
            <ChatPanel
              key={sessionManager.activeSessionId || `new-${sessionManager.activeTabIndex}`}
              messages={activeState.messages}
              isStreaming={activeState.isStreaming}
              onSend={sessionManager.sendPrompt}
              onAbort={sessionManager.abortActiveSession}
              connected={ws.connected}
              todos={activeState.todos}
              authStatus={auth.authStatus}
              onOpenAuthDialog={auth.openAuthDialog}
              pendingAnswersRef={sessionManager.pendingAnswersRef}
              draft={activeState.draft}
              onDraftChange={sessionManager.updateDraft}
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
        {activeTab === 'docs' && <DocsPanel sendRaw={ws.sendRaw} subscribe={ws.subscribe} connected={ws.connected} />}
      </div>

      <StatusBar
        connected={ws.connected}
        sessionId={sessionManager.activeSessionId}
        isStreaming={activeState.isStreaming}
        activeCount={sessionManager.activeStreamCount}
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
