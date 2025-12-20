import { Routes, Route } from 'react-router-dom';
import Layout from './components/Layout';
import Dashboard from './pages/Dashboard';
import Dns from './pages/Dns';
import Network from './pages/Network';
import Adblock from './pages/Adblock';
import Ddns from './pages/Ddns';
import Backup from './pages/Backup';
import ReverseProxy from './pages/ReverseProxy';
import Samba from './pages/Samba';
import Login from './pages/Login';

function App() {
  return (
    <Routes>
      {/* Login page - standalone, no layout */}
      <Route path="/login" element={<Login />} />

      {/* Dashboard pages with layout */}
      <Route path="/*" element={
        <Layout>
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/dns" element={<Dns />} />
            <Route path="/network" element={<Network />} />
            <Route path="/adblock" element={<Adblock />} />
            <Route path="/ddns" element={<Ddns />} />
            <Route path="/backup" element={<Backup />} />
            <Route path="/reverseproxy" element={<ReverseProxy />} />
            <Route path="/samba" element={<Samba />} />
          </Routes>
        </Layout>
      } />
    </Routes>
  );
}

export default App;
