import { useState } from 'react';
import { Rocket, ArrowRight, Loader2, CheckCircle, XCircle } from 'lucide-react';
import Button from './Button';
import { deployContainer } from '../api/client';

function DeployModal({ devContainer, prodContainer, baseDomain, onClose, onDeployStarted }) {
  const [deploying, setDeploying] = useState(false);
  const [result, setResult] = useState(null);

  async function handleDeploy() {
    setDeploying(true);
    setResult(null);
    try {
      const res = await deployContainer(devContainer.id);
      if (res.data.success !== false) {
        setResult({ type: 'success', text: 'Deploiement lance' });
        if (onDeployStarted) onDeployStarted(res.data);
        setTimeout(onClose, 1500);
      } else {
        setResult({ type: 'error', text: res.data.error || 'Erreur' });
      }
    } catch (error) {
      setResult({ type: 'error', text: error.response?.data?.error || 'Erreur de deploiement' });
    } finally {
      setDeploying(false);
    }
  }

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
      <div className="bg-gray-800 p-6 w-full max-w-md border border-gray-700">
        <div className="flex items-center gap-2 mb-4">
          <Rocket className="w-5 h-5 text-blue-400" />
          <h2 className="text-lg font-bold">Deployer en production</h2>
        </div>

        <div className="space-y-4">
          {/* Source → Target display */}
          <div className="flex items-center gap-3 p-4 bg-gray-900 border border-gray-700">
            <div className="flex-1 text-center">
              <span className="text-xs px-1.5 py-0.5 bg-blue-100 text-blue-800 font-medium">DEV</span>
              <p className="text-sm font-medium mt-1">{devContainer.name}</p>
              {baseDomain && (
                <p className="text-xs text-gray-500 font-mono mt-0.5">dev.{devContainer.slug}.{baseDomain}</p>
              )}
            </div>
            <ArrowRight className="w-5 h-5 text-gray-500 flex-shrink-0" />
            <div className="flex-1 text-center">
              <span className="text-xs px-1.5 py-0.5 bg-purple-100 text-purple-800 font-medium">PROD</span>
              <p className="text-sm font-medium mt-1">{prodContainer.name}</p>
              {baseDomain && (
                <p className="text-xs text-gray-500 font-mono mt-0.5">{prodContainer.slug}.{baseDomain}</p>
              )}
            </div>
          </div>

          <p className="text-sm text-gray-400">
            Le code du conteneur de developpement sera deploye dans le conteneur de production.
          </p>

          {/* Result */}
          {result && (
            <div className={`p-3 flex items-center gap-2 text-sm ${
              result.type === 'success' ? 'bg-green-900/50 text-green-400' : 'bg-red-900/50 text-red-400'
            }`}>
              {result.type === 'success' ? <CheckCircle className="w-4 h-4" /> : <XCircle className="w-4 h-4" />}
              {result.text}
            </div>
          )}
        </div>

        <div className="flex justify-end gap-2 mt-6">
          <Button variant="secondary" onClick={onClose} disabled={deploying}>
            Annuler
          </Button>
          <Button onClick={handleDeploy} loading={deploying} disabled={!!result?.type === 'success'}>
            {deploying ? 'Deploiement...' : 'Deployer'}
          </Button>
        </div>
      </div>
    </div>
  );
}

export default DeployModal;
