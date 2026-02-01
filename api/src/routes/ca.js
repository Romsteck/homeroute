/**
 * Routes API pour la gestion de l'Autorité de Certification locale
 */

import { Router } from 'express';
import { spawn } from 'child_process';
import { readFile } from 'fs/promises';
import path from 'path';
import { fileURLToPath } from 'url';

const router = Router();
const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Chemin vers le binaire CA (sera compilé depuis Rust)
const CA_BINARY = path.join(__dirname, '../../../ca-service/target/release/ca-cli');
const CA_STORAGE_PATH = process.env.CA_STORAGE_PATH || '/var/lib/server-dashboard/ca';

/**
 * Exécute une commande CA et retourne le résultat JSON
 */
async function execCaCommand(command, args = []) {
  return new Promise((resolve, reject) => {
    const proc = spawn(CA_BINARY, [command, ...args], {
      env: { ...process.env, CA_STORAGE_PATH },
    });

    let stdout = '';
    let stderr = '';

    proc.stdout.on('data', (data) => {
      stdout += data.toString();
    });

    proc.stderr.on('data', (data) => {
      stderr += data.toString();
    });

    proc.on('close', (code) => {
      if (code !== 0) {
        reject(new Error(`CA command failed: ${stderr || stdout}`));
        return;
      }

      try {
        const result = JSON.parse(stdout);
        resolve(result);
      } catch (e) {
        // Si pas de JSON, retourner success simple
        resolve({ success: true, message: stdout.trim() });
      }
    });

    proc.on('error', (err) => {
      reject(err);
    });
  });
}

// GET /api/ca/status - Statut de la CA
router.get('/status', async (req, res) => {
  try {
    const status = await execCaCommand('status');
    res.json({ success: true, ...status });
  } catch (error) {
    res.status(500).json({ success: false, error: error.message });
  }
});

// POST /api/ca/init - Initialiser la CA
router.post('/init', async (req, res) => {
  try {
    const result = await execCaCommand('init');
    res.json(result);
  } catch (error) {
    // Si déjà initialisée, retourner erreur appropriée
    if (error.message.includes('already initialized')) {
      res.status(409).json({ success: false, error: 'CA already initialized' });
    } else {
      res.status(500).json({ success: false, error: error.message });
    }
  }
});

// GET /api/ca/root-cert - Télécharger le certificat root
router.get('/root-cert', async (req, res) => {
  try {
    const format = req.query.format || 'pem'; // pem, crt, der
    const certPath = path.join(CA_STORAGE_PATH, 'root-ca.crt');

    const cert = await readFile(certPath);

    // Définir le content-type et le nom de fichier selon le format
    const contentTypes = {
      pem: 'application/x-pem-file',
      crt: 'application/x-x509-ca-cert',
      der: 'application/x-x509-ca-cert',
    };

    const filenames = {
      pem: 'homeroute-root-ca.pem',
      crt: 'homeroute-root-ca.crt',
      der: 'homeroute-root-ca.der',
    };

    res.setHeader('Content-Type', contentTypes[format] || contentTypes.pem);
    res.setHeader('Content-Disposition', `attachment; filename="${filenames[format] || filenames.pem}"`);

    if (format === 'der') {
      // Convertir PEM -> DER (simple extraction base64)
      const base64 = cert.toString()
        .replace(/-----BEGIN CERTIFICATE-----/, '')
        .replace(/-----END CERTIFICATE-----/, '')
        .replace(/\s/g, '');
      const der = Buffer.from(base64, 'base64');
      res.send(der);
    } else {
      res.send(cert);
    }
  } catch (error) {
    if (error.code === 'ENOENT') {
      res.status(404).json({ success: false, error: 'CA not initialized' });
    } else {
      res.status(500).json({ success: false, error: error.message });
    }
  }
});

// GET /api/ca/certificates - Liste tous les certificats émis
router.get('/certificates', async (req, res) => {
  try {
    const result = await execCaCommand('list');
    res.json(result);
  } catch (error) {
    res.status(500).json({ success: false, error: error.message });
  }
});

// POST /api/ca/issue - Émettre un nouveau certificat
router.post('/issue', async (req, res) => {
  try {
    const { domains } = req.body;

    if (!domains || !Array.isArray(domains) || domains.length === 0) {
      return res.status(400).json({ success: false, error: 'domains array required' });
    }

    // Valider les domaines
    for (const domain of domains) {
      if (typeof domain !== 'string' || domain.trim() === '') {
        return res.status(400).json({ success: false, error: `Invalid domain: ${domain}` });
      }
    }

    const result = await execCaCommand('issue', ['--domains', domains.join(',')]);
    res.json(result);
  } catch (error) {
    res.status(500).json({ success: false, error: error.message });
  }
});

// POST /api/ca/renew/:id - Renouveler un certificat
router.post('/renew/:id', async (req, res) => {
  try {
    const { id } = req.params;
    const result = await execCaCommand('renew', ['--id', id]);
    res.json(result);
  } catch (error) {
    if (error.message.includes('not found')) {
      res.status(404).json({ success: false, error: 'Certificate not found' });
    } else {
      res.status(500).json({ success: false, error: error.message });
    }
  }
});

// DELETE /api/ca/revoke/:id - Révoquer un certificat
router.delete('/revoke/:id', async (req, res) => {
  try {
    const { id } = req.params;
    const result = await execCaCommand('revoke', ['--id', id]);
    res.json(result);
  } catch (error) {
    if (error.message.includes('not found')) {
      res.status(404).json({ success: false, error: 'Certificate not found' });
    } else {
      res.status(500).json({ success: false, error: error.message });
    }
  }
});

// GET /api/ca/renewal-candidates - Certificats nécessitant un renouvellement
router.get('/renewal-candidates', async (req, res) => {
  try {
    const result = await execCaCommand('renewal-candidates');
    res.json(result);
  } catch (error) {
    res.status(500).json({ success: false, error: error.message });
  }
});

export default router;
