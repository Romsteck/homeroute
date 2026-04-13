import { Routes, Route, Navigate, useSearchParams } from 'react-router-dom';
import { AuthProvider, useAuth } from './context/AuthContext';
import { TaskProvider } from './context/TaskContext';
import { StudioProvider } from './context/StudioContext';
import Layout from './components/Layout';
import Tasks from './pages/Tasks';
import TaskDetail from './pages/TaskDetail';
import Dashboard from './pages/Dashboard';
import Dns from './pages/Dns';
import Adblock from './pages/Adblock';
import Ddns from './pages/Ddns';
import ReverseProxy from './pages/ReverseProxy';
import Updates from './pages/Updates';
import Hosts from './pages/Hosts';
import Certificates from './pages/Certificates';
import Store from './pages/Store';
import Git from './pages/Git';
import Monitoring from './pages/Monitoring';
import Login from './pages/Login';
import Profile from './pages/Profile';
import Backup from './pages/Backup';
import Energy from './pages/Energy';
import Docs from './pages/Docs';
import AppDetail from './pages/AppDetail';
import DbExplorer from './pages/DbExplorer';
import SchemaPage from './pages/SchemaPage';
import Logs from './pages/Logs';

// Component to protect routes that require authentication
function ProtectedRoute({ children }) {
  const { isAuthenticated, loading } = useAuth();

  if (loading) {
    return (
      <div className="min-h-screen bg-gray-900 flex items-center justify-center">
        <div className="text-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500 mx-auto"></div>
          <p className="mt-4 text-gray-400">Chargement...</p>
        </div>
      </div>
    );
  }

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  return children;
}

// Component to redirect authenticated users away from login
function PublicRoute({ children }) {
  const { isAuthenticated, loading } = useAuth();
  const [searchParams] = useSearchParams();
  const redirectUrl = searchParams.get('rd');

  if (loading) {
    return (
      <div className="min-h-screen bg-gray-900 flex items-center justify-center">
        <div className="text-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500 mx-auto"></div>
          <p className="mt-4 text-gray-400">Chargement...</p>
        </div>
      </div>
    );
  }

  if (isAuthenticated) {
    // If rd parameter is present, redirect to the original URL (cross-domain)
    if (redirectUrl) {
      window.location.href = redirectUrl;
      return null;
    }
    return <Navigate to="/" replace />;
  }

  return children;
}

function AppRoutes() {
  return (
    <Routes>
      {/* Public routes */}
      <Route path="/login" element={
        <PublicRoute>
          <Login />
        </PublicRoute>
      } />

      {/* Profile - protected but outside layout */}
      <Route path="/profile" element={
        <ProtectedRoute>
          <Profile />
        </ProtectedRoute>
      } />

      {/* Protected routes with Layout */}
      <Route path="/*" element={
        <ProtectedRoute>
          <TaskProvider>
          <StudioProvider>
          <Layout>
            <Routes>
              <Route path="/" element={<Dashboard />} />
              <Route path="/monitoring" element={<Monitoring />} />
              <Route path="/dns" element={<Dns />} />
              <Route path="/adblock" element={<Adblock />} />
              <Route path="/ddns" element={<Ddns />} />
              <Route path="/reverseproxy" element={<ReverseProxy />} />
              <Route path="/updates" element={<Updates />} />
              <Route path="/hosts" element={<Hosts />} />
              <Route path="/certificates" element={<Certificates />} />
              <Route path="/store" element={<Store />} />
              <Route path="/git" element={<Git />} />
              <Route path="/backup" element={<Backup />} />
              <Route path="/energy" element={<Energy />} />
              <Route path="/logs" element={<Logs />} />
              <Route path="/tasks" element={<Tasks />} />
              <Route path="/tasks/:id" element={<TaskDetail />} />
              <Route path="/studio" element={null} />
              <Route path="/apps" element={<Navigate to="/studio" replace />} />
              <Route path="/apps/:slug" element={<Navigate to="/studio" replace />} />
              <Route path="/database" element={<DbExplorer />} />
              <Route path="/schema" element={<SchemaPage />} />
              <Route path="/docs" element={<Docs />} />
              <Route path="/docs/:appId" element={<Docs />} />
            </Routes>
          </Layout>
          </StudioProvider>
          </TaskProvider>
        </ProtectedRoute>
      } />
    </Routes>
  );
}

function App() {
  return (
    <AuthProvider>
      <AppRoutes />
    </AuthProvider>
  );
}

export default App;
