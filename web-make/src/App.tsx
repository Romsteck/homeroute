import { useState, useEffect, useCallback } from 'react'
import { BrowserRouter, Routes, Route } from 'react-router-dom'
import { Layout } from './components/Layout'
import { Dashboard } from './pages/Dashboard'
import { Apps } from './pages/Apps'
import { AppDetail } from './pages/AppDetail'
import { Pipelines } from './pages/Pipelines'
import { PipelineDetail } from './pages/PipelineDetail'
import { PipelineConfigPage } from './pages/PipelineConfig'
import { DbExplorer } from './pages/DbExplorer'
import { FormBuilder } from './pages/FormBuilder'
import { fetchEnvironments } from './api'
import { useWebSocket } from './hooks/useWebSocket'
import type { Environment } from './types'

export default function App() {
  const [environments, setEnvironments] = useState<Environment[]>([])
  const [currentEnv, setCurrentEnv] = useState(() => {
    return localStorage.getItem('maker-current-env') || 'dev'
  })

  const handleEnvChange = (slug: string) => {
    setCurrentEnv(slug)
    localStorage.setItem('maker-current-env', slug)
  }

  // Apply real-time env status updates from WebSocket
  const handleEnvStatus = useCallback((data: any) => {
    if (!data?.environments) return
    setEnvironments((prev) => {
      return prev.map((env) => {
        const update = data.environments.find((u: any) => u.slug === env.slug)
        if (!update) return env
        return {
          ...env,
          status: update.status,
          agent_connected: update.agent_connected,
          agent_version: update.agent_version ?? env.agent_version,
          apps: (env.apps || []).map((app) => {
            const appUpdate = update.apps?.find((a: any) => a.slug === app.slug)
            if (!appUpdate) return app
            return { ...app, running: appUpdate.running }
          }),
        }
      })
    })
  }, [])

  useWebSocket({ 'env:status': handleEnvStatus })

  useEffect(() => {
    fetchEnvironments().then(setEnvironments)
  }, [])

  return (
    <BrowserRouter>
      <Routes>
        <Route
          element={
            <Layout
              environments={environments}
              currentEnv={currentEnv}
              onEnvChange={handleEnvChange}
            />
          }
        >
          <Route index element={<Dashboard currentEnv={currentEnv} />} />
          <Route path="/apps" element={<Apps currentEnv={currentEnv} />} />
          <Route path="/apps/:slug" element={<AppDetail />} />
          <Route path="/apps/:slug/pipeline" element={<PipelineConfigPage />} />
          <Route path="/pipelines" element={<Pipelines />} />
          <Route path="/pipelines/:id" element={<PipelineDetail />} />
          <Route path="/tables" element={<DbExplorer currentEnv={currentEnv} />} />
          <Route path="/environments/:slug/db" element={<DbExplorer />} />
          <Route path="/environments/:slug/db/:appSlug" element={<DbExplorer />} />
          <Route path="/environments/:slug/forms/:appSlug" element={<FormBuilder />} />
          <Route path="*" element={<Dashboard currentEnv={currentEnv} />} />
        </Route>
      </Routes>
    </BrowserRouter>
  )
}
