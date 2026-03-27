import { useState, useEffect } from 'react'
import { BrowserRouter, Routes, Route } from 'react-router-dom'
import { Layout } from './components/Layout'
import { Dashboard } from './pages/Dashboard'
import { Apps } from './pages/Apps'
import { AppDetail } from './pages/AppDetail'
import { Pipelines } from './pages/Pipelines'
import { Environments } from './pages/Environments'
import { EnvironmentDetail } from './pages/EnvironmentDetail'
import { DbExplorer } from './pages/DbExplorer'
import { FormBuilder } from './pages/FormBuilder'
import { fetchEnvironments } from './api'
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
          <Route path="/pipelines" element={<Pipelines />} />
          <Route path="/environments" element={<Environments />} />
          <Route path="/environments/:slug" element={<EnvironmentDetail />} />
          <Route path="/environments/:slug/db" element={<DbExplorer />} />
          <Route path="/environments/:slug/db/:appSlug" element={<DbExplorer />} />
          <Route path="/environments/:slug/forms/:appSlug" element={<FormBuilder />} />
          <Route path="*" element={<Dashboard currentEnv={currentEnv} />} />
        </Route>
      </Routes>
    </BrowserRouter>
  )
}
