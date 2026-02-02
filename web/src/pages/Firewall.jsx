import { useState, useEffect } from 'react';
import {
  Shield,
  Plus,
  Trash2,
  Power,
  ChevronDown,
  ChevronUp,
  CheckCircle,
  XCircle,
  Monitor,
  Search,
} from 'lucide-react';
import Card from '../components/Card';
import Button from '../components/Button';
import PageHeader from '../components/PageHeader';
import {
  getIpv6FirewallStatus,
  getIpv6FirewallRules,
  addIpv6FirewallRule,
  deleteIpv6FirewallRule,
  toggleIpv6FirewallRule,
  getIpv6FirewallRuleset,
  getLanClients,
} from '../api/client';

function Firewall() {
  const [status, setStatus] = useState(null);
  const [rules, setRules] = useState([]);
  const [clients, setClients] = useState([]);
  const [loading, setLoading] = useState(true);
  const [message, setMessage] = useState(null);
  const [showRuleset, setShowRuleset] = useState(false);
  const [ruleset, setRuleset] = useState('');

  // Add rule modal
  const [showAddModal, setShowAddModal] = useState(false);
  const [clientSearch, setClientSearch] = useState('');
  const [formData, setFormData] = useState({
    description: '',
    protocol: 'tcp',
    dest_port: 443,
    dest_port_end: 0,
    dest_address: '',
    source_address: '',
  });
  const [selectedClient, setSelectedClient] = useState(null);
  const [adding, setAdding] = useState(false);
  const [formError, setFormError] = useState('');

  useEffect(() => {
    loadData();
  }, []);

  async function loadData() {
    try {
      const [statusRes, rulesRes, clientsRes] = await Promise.all([
        getIpv6FirewallStatus(),
        getIpv6FirewallRules(),
        getLanClients(),
      ]);
      if (statusRes.data.success) setStatus(statusRes.data);
      if (rulesRes.data.success) setRules(rulesRes.data.rules || []);
      if (clientsRes.data.success) setClients(clientsRes.data.clients || []);
    } catch (e) {
      console.error('Failed to load firewall data:', e);
    } finally {
      setLoading(false);
    }
  }

  async function handleToggle(id) {
    try {
      const res = await toggleIpv6FirewallRule(id);
      if (res.data.success) {
        setRules(rules.map(r => r.id === id ? { ...r, enabled: res.data.enabled } : r));
        setMessage({ type: 'success', text: 'Statut mis à jour' });
      }
    } catch (e) {
      setMessage({ type: 'error', text: 'Erreur: ' + e.message });
    }
    setTimeout(() => setMessage(null), 3000);
  }

  async function handleDelete(id) {
    if (!confirm('Supprimer cette règle ?')) return;
    try {
      const res = await deleteIpv6FirewallRule(id);
      if (res.data.success) {
        setRules(rules.filter(r => r.id !== id));
        setMessage({ type: 'success', text: 'Règle supprimée' });
      }
    } catch (e) {
      setMessage({ type: 'error', text: 'Erreur: ' + e.message });
    }
    setTimeout(() => setMessage(null), 3000);
  }

  async function handleAdd(e) {
    e.preventDefault();
    setAdding(true);
    setFormError('');

    const rule = {
      id: crypto.randomUUID(),
      description: formData.description || `${formData.protocol.toUpperCase()}/${formData.dest_port}`,
      protocol: formData.protocol,
      dest_port: parseInt(formData.dest_port) || 0,
      dest_port_end: parseInt(formData.dest_port_end) || 0,
      dest_address: formData.dest_address,
      source_address: formData.source_address,
      enabled: true,
    };

    try {
      const res = await addIpv6FirewallRule(rule);
      if (res.data.success) {
        setRules([...rules, rule]);
        setShowAddModal(false);
        resetForm();
        setMessage({ type: 'success', text: 'Règle ajoutée' });
        setTimeout(() => setMessage(null), 3000);
      } else {
        setFormError(res.data.error || 'Erreur inconnue');
      }
    } catch (e) {
      setFormError(e.response?.data?.error || e.message);
    } finally {
      setAdding(false);
    }
  }

  function resetForm() {
    setFormData({ description: '', protocol: 'tcp', dest_port: 443, dest_port_end: 0, dest_address: '', source_address: '' });
    setSelectedClient(null);
    setClientSearch('');
    setFormError('');
  }

  function selectClient(client) {
    const ipv6 = client.ipv6_addresses?.[0] || '';
    setSelectedClient(client);
    setFormData(prev => ({
      ...prev,
      dest_address: ipv6,
      description: prev.description || `${client.hostname || client.mac} - ${prev.protocol.toUpperCase()}/${prev.dest_port}`,
    }));
  }

  async function toggleRuleset() {
    if (!showRuleset) {
      try {
        const res = await getIpv6FirewallRuleset();
        if (res.data.success) setRuleset(res.data.ruleset);
      } catch (e) {
        setRuleset('Erreur: ' + e.message);
      }
    }
    setShowRuleset(!showRuleset);
  }

  const filteredClients = clients.filter(c => {
    if (!clientSearch) return true;
    const q = clientSearch.toLowerCase();
    return (
      (c.hostname && c.hostname.toLowerCase().includes(q)) ||
      (c.mac && c.mac.toLowerCase().includes(q)) ||
      (c.ipv4 && c.ipv4.includes(q)) ||
      (c.ipv6_addresses && c.ipv6_addresses.some(a => a.includes(q)))
    );
  });

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-400"></div>
      </div>
    );
  }

  return (
    <div className="space-y-6 p-6">
      <PageHeader title="Firewall IPv6" icon={Shield}>
        <Button onClick={() => { resetForm(); setShowAddModal(true); }}>
          <Plus className="w-4 h-4 mr-1" />
          Ajouter une règle
        </Button>
      </PageHeader>

      {message && (
        <div className={`p-3 flex items-center gap-2 text-sm ${
          message.type === 'success' ? 'bg-green-900/50 text-green-400 border border-green-700' : 'bg-red-900/50 text-red-400 border border-red-700'
        }`}>
          {message.type === 'success' ? <CheckCircle className="w-4 h-4" /> : <XCircle className="w-4 h-4" />}
          {message.text}
        </div>
      )}

      {/* Status */}
      <Card title="Statut" icon={Shield}>
        {status ? (
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
            <div>
              <span className="text-gray-400">État</span>
              <p className={status.enabled ? 'text-green-400 font-medium' : 'text-red-400 font-medium'}>
                {status.enabled ? 'Activé' : 'Désactivé'}
              </p>
            </div>
            <div>
              <span className="text-gray-400">Préfixe protégé</span>
              <p className="text-white font-mono text-xs mt-0.5">
                {status.lan_prefix || 'Aucun'}
              </p>
            </div>
            <div>
              <span className="text-gray-400">Politique par défaut</span>
              <p className="text-orange-400 font-medium">
                {status.default_inbound_policy || 'drop'}
              </p>
            </div>
            <div>
              <span className="text-gray-400">Règles</span>
              <p className="text-white font-medium">{status.rules_count || 0}</p>
            </div>
          </div>
        ) : (
          <p className="text-gray-500">Chargement...</p>
        )}
      </Card>

      {/* Rules */}
      <Card title="Règles d'accès entrant" icon={Shield}>
        {rules.length === 0 ? (
          <div className="text-center py-8 text-gray-500">
            <Shield className="w-12 h-12 mx-auto mb-2 opacity-50" />
            <p>Aucune règle. Tout le trafic entrant IPv6 est bloqué.</p>
            <p className="text-xs mt-1">ICMPv6 (NDP, ping) est toujours autorisé.</p>
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-gray-400 border-b border-gray-700">
                <th className="pb-2">Description</th>
                <th className="pb-2">Proto</th>
                <th className="pb-2">Port</th>
                <th className="pb-2">Destination</th>
                <th className="pb-2">Source</th>
                <th className="pb-2 text-center">Actif</th>
                <th className="pb-2 text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {rules.map(rule => (
                <tr key={rule.id} className="border-b border-gray-700/50">
                  <td className="py-2.5">{rule.description || '-'}</td>
                  <td className="py-2.5">
                    <span className="px-1.5 py-0.5 bg-gray-700 text-xs">
                      {rule.protocol.toUpperCase()}
                    </span>
                  </td>
                  <td className="py-2.5 font-mono text-xs">
                    {rule.dest_port || '*'}
                    {rule.dest_port_end > 0 && `-${rule.dest_port_end}`}
                  </td>
                  <td className="py-2.5 font-mono text-xs">
                    {rule.dest_address || 'Tout le LAN'}
                  </td>
                  <td className="py-2.5 font-mono text-xs">
                    {rule.source_address || '*'}
                  </td>
                  <td className="py-2.5 text-center">
                    <button
                      onClick={() => handleToggle(rule.id)}
                      className={`w-8 h-4 rounded-full relative transition-colors ${
                        rule.enabled ? 'bg-green-600' : 'bg-gray-600'
                      }`}
                    >
                      <span className={`absolute top-0.5 w-3 h-3 bg-white rounded-full transition-transform ${
                        rule.enabled ? 'left-4' : 'left-0.5'
                      }`} />
                    </button>
                  </td>
                  <td className="py-2.5 text-right">
                    <button
                      onClick={() => handleDelete(rule.id)}
                      className="p-1 text-gray-400 hover:text-red-400 transition-colors"
                      title="Supprimer"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>

      {/* Raw ruleset */}
      <div className="bg-gray-800 border border-gray-700">
        <button
          onClick={toggleRuleset}
          className="w-full flex items-center justify-between px-4 py-3 text-sm text-gray-400 hover:text-white transition-colors"
        >
          <span>nftables ruleset (debug)</span>
          {showRuleset ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
        </button>
        {showRuleset && (
          <pre className="px-4 pb-4 text-xs text-gray-300 font-mono overflow-x-auto whitespace-pre">
            {ruleset || 'Aucun ruleset'}
          </pre>
        )}
      </div>

      {/* Add Rule Modal */}
      {showAddModal && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-gray-800 border border-gray-700 w-full max-w-2xl max-h-[90vh] overflow-y-auto">
            <div className="flex items-center justify-between p-4 border-b border-gray-700">
              <h2 className="text-lg font-bold">Ajouter une règle</h2>
              <button onClick={() => setShowAddModal(false)} className="text-gray-400 hover:text-white">
                <XCircle className="w-5 h-5" />
              </button>
            </div>

            <form onSubmit={handleAdd} className="p-4 space-y-4">
              {/* Client picker */}
              <div>
                <label className="block text-sm text-gray-400 mb-2">Sélectionner un appareil</label>
                <div className="relative mb-2">
                  <Search className="absolute left-3 top-2.5 w-4 h-4 text-gray-500" />
                  <input
                    type="text"
                    value={clientSearch}
                    onChange={e => setClientSearch(e.target.value)}
                    placeholder="Rechercher par nom, MAC, IP..."
                    className="w-full bg-gray-900 border border-gray-600 pl-9 pr-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
                  />
                </div>
                <div className="max-h-48 overflow-y-auto bg-gray-900 border border-gray-700">
                  {filteredClients.map((c, i) => {
                    const hasIpv6 = c.ipv6_addresses && c.ipv6_addresses.length > 0;
                    const isSelected = selectedClient?.mac === c.mac;
                    return (
                      <button
                        key={c.mac || i}
                        type="button"
                        onClick={() => hasIpv6 && selectClient(c)}
                        disabled={!hasIpv6}
                        className={`w-full text-left px-3 py-2 text-sm flex items-center gap-3 border-b border-gray-800 transition-colors ${
                          isSelected
                            ? 'bg-blue-900/40 border-l-2 border-l-blue-400'
                            : hasIpv6
                              ? 'hover:bg-gray-800 cursor-pointer'
                              : 'opacity-40 cursor-not-allowed'
                        }`}
                      >
                        <Monitor className="w-4 h-4 text-gray-500 flex-shrink-0" />
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <span className="font-medium text-white truncate">
                              {c.hostname || c.mac}
                            </span>
                            {c.ipv4 && <span className="text-gray-500 text-xs">{c.ipv4}</span>}
                          </div>
                          {hasIpv6 ? (
                            <span className="text-xs text-green-400 font-mono truncate block">
                              {c.ipv6_addresses[0]}
                            </span>
                          ) : (
                            <span className="text-xs text-gray-600">Pas d'IPv6 GUA</span>
                          )}
                        </div>
                        {isSelected && <CheckCircle className="w-4 h-4 text-blue-400 flex-shrink-0" />}
                      </button>
                    );
                  })}
                  {filteredClients.length === 0 && (
                    <p className="text-center text-gray-500 py-4 text-sm">Aucun appareil trouvé</p>
                  )}
                </div>
              </div>

              {/* Rule config */}
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-sm text-gray-400 mb-1">Adresse destination</label>
                  <input
                    type="text"
                    value={formData.dest_address}
                    onChange={e => setFormData(prev => ({ ...prev, dest_address: e.target.value }))}
                    placeholder="Auto ou adresse IPv6"
                    className="w-full bg-gray-900 border border-gray-600 px-3 py-2 text-sm font-mono focus:border-blue-500 focus:outline-none"
                  />
                </div>
                <div>
                  <label className="block text-sm text-gray-400 mb-1">Description</label>
                  <input
                    type="text"
                    value={formData.description}
                    onChange={e => setFormData(prev => ({ ...prev, description: e.target.value }))}
                    placeholder="Ex: HTTPS serveur web"
                    className="w-full bg-gray-900 border border-gray-600 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
                  />
                </div>
              </div>

              <div className="grid grid-cols-3 gap-3">
                <div>
                  <label className="block text-sm text-gray-400 mb-1">Protocole</label>
                  <select
                    value={formData.protocol}
                    onChange={e => setFormData(prev => ({ ...prev, protocol: e.target.value }))}
                    className="w-full bg-gray-900 border border-gray-600 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
                  >
                    <option value="tcp">TCP</option>
                    <option value="udp">UDP</option>
                    <option value="any">Any</option>
                  </select>
                </div>
                <div>
                  <label className="block text-sm text-gray-400 mb-1">Port</label>
                  <input
                    type="number"
                    value={formData.dest_port}
                    onChange={e => setFormData(prev => ({ ...prev, dest_port: e.target.value }))}
                    placeholder="443"
                    className="w-full bg-gray-900 border border-gray-600 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
                  />
                </div>
                <div>
                  <label className="block text-sm text-gray-400 mb-1">Source</label>
                  <input
                    type="text"
                    value={formData.source_address}
                    onChange={e => setFormData(prev => ({ ...prev, source_address: e.target.value }))}
                    placeholder="Toutes"
                    className="w-full bg-gray-900 border border-gray-600 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
                  />
                </div>
              </div>

              {formError && (
                <div className="p-2 bg-red-900/50 text-red-400 text-sm border border-red-700">
                  {formError}
                </div>
              )}

              <div className="flex gap-2 pt-2">
                <Button variant="secondary" type="button" onClick={() => setShowAddModal(false)}>
                  Annuler
                </Button>
                <Button type="submit" disabled={adding}>
                  {adding ? 'Ajout...' : 'Ajouter la règle'}
                </Button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}

export default Firewall;
