import { useState, useEffect, useMemo, useCallback } from 'react';
import useStudioWebSocket from './hooks/useStudioWebSocket';
import useSessionTabs from './hooks/useSessionTabs';
import useClaudeAuth from './hooks/useClaudeAuth';
import useAppStatus from './hooks/useAppStatus';
import Header from './components/Header';
import SessionTabs from './components/SessionTabs';
import ChatPanel from './components/ChatPanel';
import TodoPanel from './components/TodoPanel';
import PreviewPanel from './components/PreviewPanel';
import CodeServerPanel from './components/CodeServerPanel';
import FilesPanel from './components/FilesPanel';
import DocsPanel from './components/DocsPanel';
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
  const [codeServerOpened, setCodeServerOpened] = useState(() => activeTab === 'code');
  const ws = useStudioWebSocket();
  const sessionManager = useSessionTabs(ws);
  const auth = useClaudeAuth(ws.subscribe, ws.sendRaw, ws.connected);
  const appStatus = useAppStatus();
  const appInfo = useMemo(() => getAppInfo(), []);
  const activeState = sessionManager.getActiveState();

  useEffect(() => {
    document.title = `Studio - ${appInfo.appName}`;
  }, [appInfo.appName]);

  const handleTabChange = useCallback((tab) => {
    setActiveTab(tab);
    localStorage.setItem('studio-active-tab', tab);
    if (tab === 'code') setCodeServerOpened(true);
  }, []);

  return (
    <div className="h-screen flex flex-col bg-gray-900">
      <Header
        appName={appInfo.appName}
        activeTab={activeTab}
        onTabChange={handleTabChange}
        connected={ws.connected}
        authStatus={auth.authStatus}
        onOpenAuthDialog={auth.openAuthDialog}
        apps={appStatus.apps}
        onStartApp={appStatus.startApp}
        onStopApp={appStatus.stopApp}
        onRestartApp={appStatus.restartApp}
        onStartAll={appStatus.startAll}
        onStopAll={appStatus.stopAll}
        onFetchLogs={appStatus.fetchLogs}
      />

      {/* Session tabs - only visible in studio mode */}
      {activeTab === 'studio' && (
        <SessionTabs
          tabs={sessionManager.tabs}
          activeIndex={sessionManager.activeTabIndex}
          onSwitch={sessionManager.switchTab}
          onClose={sessionManager.closeTab}
          onNew={sessionManager.newTab}
          sessions={ws.sessions}
          currentSessionId={sessionManager.activeSessionId}
          onSelectSession={(id, label, sessionType) => sessionManager.openTab(id, label, sessionType)}
          onDeleteSession={ws.deleteSession}
        />
      )}

      <div className="flex flex-1 min-h-0 relative">
        {/* Studio tab - always mounted, hidden when inactive */}
        <div className="flex flex-1 min-h-0" style={{display: activeTab === 'studio' ? 'flex' : 'none'}}>
          <div className="w-[30%] min-w-[300px] flex flex-col border-r border-gray-800 min-h-0">
            {/* Agent chat panel */}
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
        {/* Code Server tab - lazy-loaded on first click, then kept alive invisible */}
        {codeServerOpened && (
          <div className="flex-1" style={activeTab === 'code'
            ? { display: 'flex' }
            : { position: 'absolute', inset: 0, visibility: 'hidden', pointerEvents: 'none' }
          }>
            <CodeServerPanel slug={appInfo.slug} domain={appInfo.domain} />
          </div>
        )}
        {/* Files tab - only mounted when active */}
        {activeTab === 'files' && <FilesPanel sendRaw={ws.sendRaw} subscribe={ws.subscribe} connected={ws.connected} />}
        {activeTab === 'docs' && <DocsPanel sendRaw={ws.sendRaw} subscribe={ws.subscribe} connected={ws.connected} />}
      </div>


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
