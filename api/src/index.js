import express from 'express';
import cors from 'cors';
import dotenv from 'dotenv';
import path from 'path';
import { fileURLToPath } from 'url';
import { createServer } from 'http';
import { Server } from 'socket.io';
import { connectDB } from './config/db.js';
import { setIO } from './socket.js';

// Routes
import dnsRoutes from './routes/dns.js';
import networkRoutes from './routes/network.js';
import natRoutes from './routes/nat.js';
import adblockRoutes from './routes/adblock.js';
import ddnsRoutes from './routes/ddns.js';
import backupRoutes from './routes/backup.js';
import reverseproxyRoutes from './routes/reverseproxy.js';
import sambaRoutes from './routes/samba.js';

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

// Middleware
app.use(cors());
app.use(express.json());

// Connect to MongoDB
connectDB();

// Routes
app.use('/api/dns', dnsRoutes);
app.use('/api/network', networkRoutes);
app.use('/api/nat', natRoutes);
app.use('/api/adblock', adblockRoutes);
app.use('/api/ddns', ddnsRoutes);
app.use('/api/backup', backupRoutes);
app.use('/api/reverseproxy', reverseproxyRoutes);
app.use('/api/samba', sambaRoutes);

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
