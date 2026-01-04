import express from 'express';
import cors from 'cors';
import session from 'express-session';
import dotenv from 'dotenv';
import path from 'path';
import { fileURLToPath } from 'url';
import { createServer } from 'http';
import { Server } from 'socket.io';
import { connectDB } from './config/db.js';
import { setIO } from './socket.js';
import { getBaseDomain } from './services/reverseproxy.js';

// Routes
import dnsRoutes from './routes/dns.js';
import networkRoutes from './routes/network.js';
import natRoutes from './routes/nat.js';
import adblockRoutes from './routes/adblock.js';
import ddnsRoutes from './routes/ddns.js';
import backupRoutes from './routes/backup.js';
import reverseproxyRoutes from './routes/reverseproxy.js';
import sambaRoutes from './routes/samba.js';
import authRoutes from './routes/auth.js';
import updatesRoutes from './routes/updates.js';
import energyRoutes from './routes/energy.js';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Load env from parent directory
dotenv.config({ path: '../.env' });

const app = express();
const httpServer = createServer(app);
const io = new Server(httpServer, {
  cors: {
    origin: '*',
    methods: ['GET', 'POST']
  }
});
const PORT = process.env.PORT || 3001;

// Set io instance for use in other modules
setIO(io);

// Trust proxy (Caddy) pour les headers X-Forwarded-*
app.set('trust proxy', 1);

// Middleware
app.use(cors({
  origin: true,
  credentials: true  // Permet les cookies cross-origin
}));
app.use(express.json());

// Demarrage async pour attendre la config session
async function startServer() {
  // Session middleware (SSO via cookie partage entre sous-domaines)
  const baseDomain = await getBaseDomain();
  app.use(session({
    secret: process.env.SESSION_SECRET || 'server-dashboard-secret-key-change-in-production',
    name: 'dashboard.sid',
    resave: false,
    saveUninitialized: false,
    cookie: {
      domain: baseDomain ? `.${baseDomain}` : undefined,
      secure: true,  // Toujours secure car derriere Caddy HTTPS
      httpOnly: true,
      maxAge: 24 * 60 * 60 * 1000, // 24h
      sameSite: 'lax'
    },
    proxy: true  // Faire confiance au proxy (Caddy) pour X-Forwarded-Proto
  }));

  // Connect to MongoDB
  connectDB();

  // Routes
  app.use('/api/auth', authRoutes);
  app.use('/api/dns', dnsRoutes);
  app.use('/api/network', networkRoutes);
  app.use('/api/nat', natRoutes);
  app.use('/api/adblock', adblockRoutes);
  app.use('/api/ddns', ddnsRoutes);
  app.use('/api/backup', backupRoutes);
  app.use('/api/reverseproxy', reverseproxyRoutes);
  app.use('/api/samba', sambaRoutes);
  app.use('/api/updates', updatesRoutes);
  app.use('/api/energy', energyRoutes);

  // Health check
  app.get('/api/health', (req, res) => {
    res.json({ status: 'ok', timestamp: new Date().toISOString() });
  });

  // Servir les fichiers statiques du frontend en production
  const distPath = path.join(__dirname, '../../web/dist');
  app.use(express.static(distPath));

  // Fallback pour SPA routing
  app.get('*', (req, res) => {
    res.sendFile(path.join(distPath, 'index.html'));
  });

  httpServer.listen(PORT, () => {
    console.log(`API server running on http://localhost:${PORT}`);
  });
}

startServer();
