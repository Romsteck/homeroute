import { useState, useEffect, useCallback } from "react";
import type { Environment, TabId } from "./types";
import { getEnvironment } from "./api";
import { StudioLayout } from "./components/StudioLayout";

function parseEnvSlug(): string {
  const hostname = window.location.hostname;
  // studio.{env}.mynetwk.biz
  const parts = hostname.split(".");
  if (parts.length >= 3 && parts[0] === "studio") {
    return parts[1];
  }
  // Fallback for local development
  return "dev";
}

function getInitialApp(): string | null {
  // Check URL param ?folder=/apps/{slug}
  const params = new URLSearchParams(window.location.search);
  const folder = params.get("folder");
  if (folder) {
    const match = folder.match(/\/apps\/([^/]+)/);
    if (match) return match[1];
  }
  // Check localStorage
  return localStorage.getItem("studio:selectedApp");
}

function getInitialTab(): TabId {
  return (localStorage.getItem("studio:activeTab") as TabId) || "code";
}

export default function App() {
  const [envSlug] = useState(parseEnvSlug);
  const [env, setEnv] = useState<Environment | null>(null);
  const [selectedApp, setSelectedApp] = useState<string | null>(getInitialApp);
  const [activeTab, setActiveTab] = useState<TabId>(getInitialTab);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setLoading(true);
    getEnvironment(envSlug)
      .then((data) => {
        setEnv(data);
        // Auto-select first app if none selected
        if (!selectedApp && data.apps.length > 0) {
          setSelectedApp(data.apps[0].slug);
        }
        setError(null);
      })
      .catch((err) => {
        // Fallback mock data for development
        const mockEnv: Environment = {
          slug: envSlug,
          name: envSlug.toUpperCase(),
          type: envSlug === "prod" ? "prod" : envSlug === "acc" ? "acc" : "dev",
          code_server_ip: "10.0.0.10",
          code_server_port: 8443,
          apps: [
            { slug: "trader", name: "Trader", container_ip: "10.0.0.122", status: "running", stack: "axum+vite", port: 3000 },
            { slug: "wallet", name: "Wallet", container_ip: "10.0.0.103", status: "running", stack: "axum+vite", port: 3000 },
            { slug: "home", name: "Home", container_ip: "10.0.0.127", status: "running", stack: "axum+vite", port: 3000 },
            { slug: "files", name: "Files", container_ip: "10.0.0.109", status: "running", stack: "axum+vite", port: 3000 },
            { slug: "padel", name: "Padel", container_ip: "10.0.0.117", status: "running", stack: "nextjs", port: 3000 },
            { slug: "www", name: "WWW", container_ip: "10.0.0.125", status: "stopped", stack: "nextjs", port: 3000 },
            { slug: "forge", name: "Forge", container_ip: "10.0.0.128", status: "running", stack: "nextjs", port: 3000 },
            { slug: "aptymus", name: "Aptymus", container_ip: "10.0.0.119", status: "running", stack: "nextjs", port: 3000 },
            { slug: "myfrigo", name: "MyFrigo", container_ip: "10.0.0.126", status: "running", stack: "axum+flutter", port: 3000 },
            { slug: "calendar", name: "Calendar", container_ip: "10.0.0.110", status: "stopped", stack: "nextjs", port: 3000 },
          ],
        };
        setEnv(mockEnv);
        if (!selectedApp) {
          setSelectedApp(mockEnv.apps[0].slug);
        }
        setError(`Using mock data (API unavailable: ${err.message})`);
      })
      .finally(() => setLoading(false));
  }, [envSlug]);

  useEffect(() => {
    if (selectedApp) {
      localStorage.setItem("studio:selectedApp", selectedApp);
    }
  }, [selectedApp]);

  useEffect(() => {
    localStorage.setItem("studio:activeTab", activeTab);
  }, [activeTab]);

  const handleSelectApp = useCallback((slug: string) => {
    setSelectedApp(slug);
  }, []);

  const handleSelectTab = useCallback((tab: TabId) => {
    setActiveTab(tab);
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center min-h-screen" style={{ background: "var(--bg-primary)" }}>
        <div className="text-center">
          <div
            className="w-8 h-8 border-2 rounded-full animate-spin mx-auto mb-4"
            style={{ borderColor: "var(--border)", borderTopColor: "var(--accent)" }}
          />
          <p style={{ color: "var(--text-secondary)" }}>Loading Studio...</p>
        </div>
      </div>
    );
  }

  if (!env) {
    return (
      <div className="flex items-center justify-center min-h-screen" style={{ background: "var(--bg-primary)" }}>
        <p style={{ color: "var(--danger)" }}>Failed to load environment</p>
      </div>
    );
  }

  const currentApp = env.apps.find((a) => a.slug === selectedApp) || null;

  return (
    <StudioLayout
      env={env}
      currentApp={currentApp}
      apps={env.apps}
      selectedApp={selectedApp}
      activeTab={activeTab}
      onSelectApp={handleSelectApp}
      onSelectTab={handleSelectTab}
      error={error}
    />
  );
}
