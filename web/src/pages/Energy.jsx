import { useState, useEffect, useRef } from 'react';
import { Zap, Thermometer, Cpu, Clock, Moon, Rocket, Play, Square, Activity } from 'lucide-react';
import Button from '../components/Button';
import PageHeader from '../components/PageHeader';
import Section from '../components/Section';
import {
  getCpuInfo,
  getCurrentEnergyMode,
  setEnergyMode,
  getEnergySchedule,
  saveEnergySchedule,
  getAutoSelectConfig,
  saveAutoSelectConfig,
  getSelectableInterfaces,
  getBenchmarkStatus,
  startBenchmark,
  stopBenchmark
} from '../api/client';

const MODE_ICONS = {
  economy: Moon,
  auto: Zap,
  performance: Rocket
};

const MODE_LABELS = {
  economy: 'Économie',
  auto: 'Auto',
  performance: 'Performance'
};

const MODE_DESCRIPTIONS = {
  economy: 'CPU limité à 60% + économie maximale',
  auto: 'CPU limité à 85% + équilibré',
  performance: 'CPU pleine puissance'
};

const MODE_COLORS = {
  economy: { bg: 'bg-indigo-600', ring: 'ring-indigo-400', hover: 'hover:bg-indigo-700' },
  auto: { bg: 'bg-blue-600', ring: 'ring-blue-400', hover: 'hover:bg-blue-700' },
  performance: { bg: 'bg-orange-600', ring: 'ring-orange-400', hover: 'hover:bg-orange-700' }
};

function Energy() {
  // CPU state
  const [cpuInfo, setCpuInfo] = useState({ temperature: null, frequency: null, usage: null });
  const [cpuModel, setCpuModel] = useState('CPU');

  // Mode state
  const [currentMode, setCurrentMode] = useState('auto');
  const [changingMode, setChangingMode] = useState(false);

  // Schedule state
  const [schedule, setSchedule] = useState({
    enabled: false,
    nightStart: '00:00',
    nightEnd: '08:00'
  });
  const [savingSchedule, setSavingSchedule] = useState(false);
  const [scheduleMessage, setScheduleMessage] = useState(null);

  // Auto-select state
  const [autoSelect, setAutoSelect] = useState({
    enabled: false,
    networkInterface: null,
    thresholds: { low: 1000, high: 10000 },
    averagingTime: 3,
    sampleInterval: 1000
  });
  const [savingAutoSelect, setSavingAutoSelect] = useState(false);
  const [autoSelectMessage, setAutoSelectMessage] = useState(null);
  const [networkRps, setNetworkRps] = useState({ current: 0, averaged: 0, appliedMode: null });
  const [interfaces, setInterfaces] = useState([]);
  const [interfaceError, setInterfaceError] = useState(null);

  // Benchmark state
  const [benchmark, setBenchmark] = useState({ running: false, elapsed: 0 });

  const [loading, setLoading] = useState(true);
  const pollingRef = useRef(null);

  // Initial data fetch
  useEffect(() => {
    async function fetchInitialData() {
      try {
        const [modeRes, scheduleRes, autoSelectRes, interfacesRes] = await Promise.all([
          getCurrentEnergyMode(),
          getEnergySchedule(),
          getAutoSelectConfig(),
          getSelectableInterfaces()
        ]);

        if (modeRes.data.success) {
          setCurrentMode(modeRes.data.mode);
        }

        if (scheduleRes.data.success) {
          setSchedule(scheduleRes.data.config);
        }

        if (autoSelectRes.data.success) {
          setAutoSelect(autoSelectRes.data.config);
        }

        if (interfacesRes.data.success) {
          setInterfaces(interfacesRes.data.interfaces);
        }
      } catch (error) {
        console.error('Error fetching initial data:', error);
      } finally {
        setLoading(false);
      }
    }

    fetchInitialData();
  }, []);

  // SSE connection for real-time mode and RPS updates
  useEffect(() => {
    const eventSource = new EventSource('/api/energy/events');

    eventSource.addEventListener('modeChange', (e) => {
      const data = JSON.parse(e.data);
      setCurrentMode(data.mode);
    });

    eventSource.addEventListener('rpsUpdate', (e) => {
      const data = JSON.parse(e.data);
      setNetworkRps({
        current: data.rps || 0,
        averaged: data.averagedRps || 0,
        appliedMode: data.appliedMode || null
      });
      setInterfaceError(data.interfaceError || null);
    });

    eventSource.onerror = () => {
      console.error('SSE connection error, reconnecting...');
    };

    return () => {
      eventSource.close();
    };
  }, []);

  // CPU and benchmark polling (less frequent, SSE handles mode/RPS)
  useEffect(() => {
    async function pollData() {
      try {
        const [cpuRes, benchRes] = await Promise.all([
          getCpuInfo(),
          getBenchmarkStatus()
        ]);

        if (cpuRes.data.success) {
          setCpuInfo({
            temperature: cpuRes.data.temperature,
            frequency: cpuRes.data.frequency,
            usage: cpuRes.data.usage
          });
          if (cpuRes.data.model) {
            setCpuModel(cpuRes.data.model);
          }
        }

        if (benchRes.data.success) {
          setBenchmark({
            running: benchRes.data.running,
            elapsed: benchRes.data.elapsed || 0
          });
        }
      } catch (error) {
        console.error('Error polling data:', error);
      }
    }

    pollData();
    pollingRef.current = setInterval(pollData, 2000); // Slower polling, SSE handles real-time

    return () => {
      if (pollingRef.current) {
        clearInterval(pollingRef.current);
      }
    };
  }, []);

  // Mode change handler
  async function handleModeChange(mode) {
    if (mode === currentMode) return;

    setChangingMode(true);
    try {
      const res = await setEnergyMode(mode);
      if (res.data.success) {
        setCurrentMode(mode);
      }
    } catch (error) {
      console.error('Error changing mode:', error);
    } finally {
      setChangingMode(false);
    }
  }

  // Benchmark handlers
  async function handleStartBenchmark() {
    try {
      const res = await startBenchmark(60);
      if (res.data.success) {
        setBenchmark({ running: true, elapsed: 0 });
      }
    } catch (error) {
      console.error('Error starting benchmark:', error);
    }
  }

  async function handleStopBenchmark() {
    try {
      await stopBenchmark();
      setBenchmark({ running: false, elapsed: 0 });
    } catch (error) {
      console.error('Error stopping benchmark:', error);
    }
  }

  // Schedule save handler
  async function handleSaveSchedule() {
    setSavingSchedule(true);
    setScheduleMessage(null);
    try {
      const res = await saveEnergySchedule(schedule);
      if (res.data.success) {
        setScheduleMessage({ type: 'success', text: 'Programmation enregistrée' });
      } else {
        setScheduleMessage({ type: 'error', text: res.data.error || 'Erreur' });
      }
    } catch (error) {
      setScheduleMessage({ type: 'error', text: error.message });
    } finally {
      setSavingSchedule(false);
      setTimeout(() => setScheduleMessage(null), 3000);
    }
  }

  // Auto-select save handler
  async function handleSaveAutoSelect() {
    setSavingAutoSelect(true);
    setAutoSelectMessage(null);
    try {
      const res = await saveAutoSelectConfig(autoSelect);
      if (res.data.success) {
        setAutoSelectMessage({ type: 'success', text: 'Configuration enregistrée' });
      } else {
        setAutoSelectMessage({ type: 'error', text: res.data.error || 'Erreur' });
      }
    } catch (error) {
      setAutoSelectMessage({ type: 'error', text: error.message });
    } finally {
      setSavingAutoSelect(false);
      setTimeout(() => setAutoSelectMessage(null), 3000);
    }
  }

  // Temperature color helper
  function getTempColor(temp) {
    if (temp < 50) return 'text-green-400';
    if (temp < 70) return 'text-yellow-400';
    if (temp < 85) return 'text-orange-400';
    return 'text-red-400';
  }

  function getTempBarColor(temp) {
    if (temp < 50) return 'bg-green-500';
    if (temp < 70) return 'bg-yellow-500';
    if (temp < 85) return 'bg-orange-500';
    return 'bg-red-500';
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-400"></div>
      </div>
    );
  }

  return (
    <div>
      <PageHeader title="Énergie" icon={Zap} />

      <Section title={`CPU — ${cpuModel}`}>
        <div className="flex items-center gap-6 text-sm">
          <div className="flex items-center gap-1.5">
            <Thermometer className="w-3.5 h-3.5 text-gray-500" />
            <span className={`font-semibold ${getTempColor(cpuInfo.temperature || 0)}`}>
              {cpuInfo.temperature !== null ? `${cpuInfo.temperature.toFixed(0)}°C` : '--'}
            </span>
            <div className="w-16 bg-gray-700 h-1.5 overflow-hidden">
              <div
                className={`h-full ${getTempBarColor(cpuInfo.temperature || 0)} transition-all`}
                style={{ width: `${Math.min(100, ((cpuInfo.temperature || 0) / 95) * 100)}%` }}
              />
            </div>
          </div>
          <div className="flex items-center gap-1.5">
            <Zap className="w-3.5 h-3.5 text-gray-500" />
            <span className="font-semibold text-blue-400">
              {cpuInfo.frequency?.current != null ? `${cpuInfo.frequency.current.toFixed(1)} GHz` : '--'}
            </span>
            <span className="text-xs text-gray-500">
              {cpuInfo.frequency?.min && cpuInfo.frequency?.max
                ? `(${cpuInfo.frequency.min.toFixed(1)}-${cpuInfo.frequency.max.toFixed(1)})`
                : ''}
            </span>
          </div>
          <div className="flex items-center gap-1.5">
            <Cpu className="w-3.5 h-3.5 text-gray-500" />
            <span className="font-semibold text-purple-400">
              {cpuInfo.usage !== null ? `${cpuInfo.usage.toFixed(0)}%` : '--'}
            </span>
            <div className="w-16 bg-gray-700 h-1.5 overflow-hidden">
              <div className="h-full bg-purple-500 transition-all" style={{ width: `${cpuInfo.usage || 0}%` }} />
            </div>
          </div>
          <div className="flex items-center gap-2 ml-auto">
            <span className="text-xs text-gray-500">
              {benchmark.running ? `Benchmark ${benchmark.elapsed}s/60s` : ''}
            </span>
            {benchmark.running ? (
              <Button onClick={handleStopBenchmark} variant="danger" size="sm">
                <Square className="w-3.5 h-3.5" /> Stop
              </Button>
            ) : (
              <Button onClick={handleStartBenchmark} variant="success" size="sm">
                <Play className="w-3.5 h-3.5" /> Bench
              </Button>
            )}
          </div>
        </div>
      </Section>

      <Section title="Mode" contrast>
        <div className="flex items-center gap-px">
          {['economy', 'auto', 'performance'].map(mode => {
            const Icon = MODE_ICONS[mode];
            const isActive = currentMode === mode;
            const colors = MODE_COLORS[mode];
            const isDisabled = changingMode || autoSelect.enabled;

            return (
              <button
                key={mode}
                onClick={() => handleModeChange(mode)}
                disabled={isDisabled}
                className={`flex items-center gap-2 px-4 py-2 text-sm transition-all ${
                  isActive
                    ? `${colors.bg} text-white ring-1 ${colors.ring}`
                    : `bg-gray-800 text-gray-300 ${isDisabled ? '' : colors.hover}`
                } ${isDisabled ? 'opacity-50 cursor-not-allowed' : ''}`}
              >
                <Icon size={16} className={isActive ? 'text-white' : 'text-gray-400'} />
                <span className="font-medium">{MODE_LABELS[mode]}</span>
                {isActive && (
                  <span className="text-xs opacity-75">({autoSelect.enabled ? 'auto' : 'actif'})</span>
                )}
              </button>
            );
          })}
          <span className="ml-3 text-sm text-gray-400">{MODE_DESCRIPTIONS[currentMode]}</span>
        </div>
      </Section>

      <Section title="Programmation">
        <div className="space-y-2">
          <div className="flex items-center gap-3">
            <button
              onClick={async () => {
                const newSchedule = { ...schedule, enabled: !schedule.enabled };
                setSchedule(newSchedule);
                try {
                  await saveEnergySchedule(newSchedule);
                } catch (error) {
                  console.error('Error saving schedule:', error);
                }
              }}
              className={`relative w-10 h-5 transition-colors ${
                schedule.enabled ? 'bg-blue-600' : 'bg-gray-600'
              }`}
            >
              <span
                className={`absolute top-0.5 left-0.5 w-4 h-4 bg-white transition-transform ${
                  schedule.enabled ? 'translate-x-5' : ''
                }`}
              />
            </button>
            <span className="text-sm text-gray-300 font-medium">Programmation horaire</span>
          </div>

          {schedule.enabled && (
            <div className="flex flex-wrap items-center gap-2 text-sm">
              <span className="text-gray-400">Forcer économie de</span>
              <input
                type="time"
                value={schedule.nightStart}
                onChange={e => setSchedule(prev => ({ ...prev, nightStart: e.target.value }))}
                className="bg-gray-800 border border-gray-700 px-2 py-1 text-sm text-white"
              />
              <span className="text-gray-400">à</span>
              <input
                type="time"
                value={schedule.nightEnd}
                onChange={e => setSchedule(prev => ({ ...prev, nightEnd: e.target.value }))}
                className="bg-gray-800 border border-gray-700 px-2 py-1 text-sm text-white"
              />
              <Button onClick={handleSaveSchedule} loading={savingSchedule} size="sm">
                Enregistrer
              </Button>
              {scheduleMessage && (
                <span className={`text-sm ${scheduleMessage.type === 'success' ? 'text-green-400' : 'text-red-400'}`}>
                  {scheduleMessage.text}
                </span>
              )}
            </div>
          )}
        </div>
      </Section>

      <Section title="Auto-select" contrast>
        <div className="space-y-2">
          <div className="flex items-center gap-3">
            <button
              onClick={async () => {
                if (!autoSelect.enabled && !autoSelect.networkInterface) {
                  setAutoSelectMessage({ type: 'error', text: 'Sélectionnez d\'abord une interface' });
                  setTimeout(() => setAutoSelectMessage(null), 3000);
                  return;
                }
                const newAutoSelect = { ...autoSelect, enabled: !autoSelect.enabled };
                setAutoSelect(newAutoSelect);
                try {
                  const res = await saveAutoSelectConfig(newAutoSelect);
                  if (!res.data.success) {
                    setAutoSelect(autoSelect);
                    setAutoSelectMessage({ type: 'error', text: res.data.error });
                    setTimeout(() => setAutoSelectMessage(null), 3000);
                  }
                } catch (error) {
                  setAutoSelect(autoSelect);
                  console.error('Error saving auto-select config:', error);
                }
              }}
              className={`relative w-10 h-5 transition-colors ${
                autoSelect.enabled ? 'bg-green-600' : 'bg-gray-600'
              }`}
            >
              <span
                className={`absolute top-0.5 left-0.5 w-4 h-4 bg-white transition-transform ${
                  autoSelect.enabled ? 'translate-x-5' : ''
                }`}
              />
            </button>
            <span className="text-sm text-gray-300 font-medium">Sélection automatique</span>
            {autoSelectMessage && !autoSelect.enabled && (
              <span className={`text-xs ${autoSelectMessage.type === 'success' ? 'text-green-400' : 'text-red-400'}`}>
                {autoSelectMessage.text}
              </span>
            )}
          </div>

          <div className="flex items-center gap-3 text-sm">
            <Activity className="w-3.5 h-3.5 text-gray-500" />
            {interfaces.length === 0 ? (
              <span className="text-gray-500">Aucune interface réseau détectée</span>
            ) : (
              <select
                value={autoSelect.networkInterface || ''}
                onChange={e => setAutoSelect(prev => ({
                  ...prev,
                  networkInterface: e.target.value || null
                }))}
                className="bg-gray-800 border border-gray-700 px-2 py-1 text-sm text-white"
              >
                <option value="">Sélectionner une interface...</option>
                {interfaces.map(iface => (
                  <option key={iface.name} value={iface.name}>
                    {iface.name} ({iface.primaryIp}){iface.state !== 'UP' ? ' - DOWN' : ''}
                  </option>
                ))}
              </select>
            )}
            {interfaceError === 'not_configured' && (
              <span className="text-yellow-400 text-xs">Sélectionnez une interface</span>
            )}
            {interfaceError === 'not_found' && (
              <span className="text-red-400 text-xs">Interface introuvable</span>
            )}
            {autoSelect.networkInterface && !interfaceError && (
              <>
                <span className="text-gray-400">Charge:</span>
                <span className="text-white font-mono font-semibold">{networkRps.averaged.toLocaleString()} req/s</span>
                {autoSelect.enabled && currentMode && (
                  <span className={`font-medium ${
                    currentMode === 'economy' ? 'text-indigo-400' :
                    currentMode === 'performance' ? 'text-orange-400' : 'text-blue-400'
                  }`}>
                    → {MODE_LABELS[currentMode]}
                  </span>
                )}
              </>
            )}
          </div>

          {autoSelect.enabled && (
            <div className="flex items-center gap-2 text-sm">
              <div>
                <label className="block text-xs text-gray-500 mb-0.5">Seuil bas</label>
                <input
                  type="number"
                  value={autoSelect.thresholds.low}
                  onChange={e => setAutoSelect(prev => ({
                    ...prev,
                    thresholds: { ...prev.thresholds, low: parseInt(e.target.value) || 0 }
                  }))}
                  className="w-24 bg-gray-800 border border-gray-700 px-2 py-1 text-sm text-white"
                />
              </div>
              <div>
                <label className="block text-xs text-gray-500 mb-0.5">Seuil haut</label>
                <input
                  type="number"
                  value={autoSelect.thresholds.high}
                  onChange={e => setAutoSelect(prev => ({
                    ...prev,
                    thresholds: { ...prev.thresholds, high: parseInt(e.target.value) || 0 }
                  }))}
                  className="w-24 bg-gray-800 border border-gray-700 px-2 py-1 text-sm text-white"
                />
              </div>
              <div>
                <label className="block text-xs text-gray-500 mb-0.5">Moyenne (s)</label>
                <input
                  type="number"
                  min="1"
                  max="30"
                  value={autoSelect.averagingTime}
                  onChange={e => setAutoSelect(prev => ({
                    ...prev,
                    averagingTime: Math.max(1, Math.min(30, parseInt(e.target.value) || 3))
                  }))}
                  className="w-16 bg-gray-800 border border-gray-700 px-2 py-1 text-sm text-white"
                />
              </div>
              <div className="self-end">
                <Button onClick={handleSaveAutoSelect} loading={savingAutoSelect} size="sm">
                  Enregistrer
                </Button>
              </div>
              {autoSelectMessage && (
                <span className={`self-end text-xs ${autoSelectMessage.type === 'success' ? 'text-green-400' : 'text-red-400'}`}>
                  {autoSelectMessage.text}
                </span>
              )}
            </div>
          )}
        </div>
      </Section>
    </div>
  );
}

export default Energy;
