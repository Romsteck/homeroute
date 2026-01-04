import { useState, useEffect, useRef, useCallback } from 'react';
import { Zap, Thermometer, Cpu, Fan, Clock, Moon, Rocket } from 'lucide-react';
import Card from '../components/Card';
import Button from '../components/Button';
import FanCurveEditor from '../components/FanCurveEditor';
import {
  getCpuInfo,
  getCurrentEnergyMode,
  setEnergyMode,
  getFansStatus,
  getFanProfiles,
  saveFanProfile,
  getEnergySchedule,
  saveEnergySchedule
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
  economy: 'CPU lent + ventilos silencieux',
  auto: 'Adaptatif selon la charge',
  performance: 'CPU max + ventilos actifs'
};

const MODE_COLORS = {
  economy: { bg: 'bg-indigo-600', ring: 'ring-indigo-400', hover: 'hover:bg-indigo-700' },
  auto: { bg: 'bg-blue-600', ring: 'ring-blue-400', hover: 'hover:bg-blue-700' },
  performance: { bg: 'bg-orange-600', ring: 'ring-orange-400', hover: 'hover:bg-orange-700' }
};

function Energy() {
  // CPU state
  const [cpuInfo, setCpuInfo] = useState({ temperature: null, frequency: null, usage: null });

  // Mode state
  const [currentMode, setCurrentMode] = useState('auto');
  const [changingMode, setChangingMode] = useState(false);

  // Fans state
  const [fans, setFans] = useState([]);
  const [fansAvailable, setFansAvailable] = useState(false);
  const [fanProfiles, setFanProfiles] = useState([]);
  const [editingProfile, setEditingProfile] = useState('silent');
  const [savingProfiles, setSavingProfiles] = useState(false);
  const [profilesModified, setProfilesModified] = useState(false);

  // Schedule state
  const [schedule, setSchedule] = useState({
    enabled: false,
    nightStart: '22:00',
    nightEnd: '08:00',
    dayMode: 'auto',
    nightMode: 'economy'
  });
  const [savingSchedule, setSavingSchedule] = useState(false);
  const [scheduleMessage, setScheduleMessage] = useState(null);

  const [loading, setLoading] = useState(true);
  const pollingRef = useRef(null);

  // Initial data fetch
  useEffect(() => {
    async function fetchInitialData() {
      try {
        const [modeRes, fansRes, profilesRes, scheduleRes] = await Promise.all([
          getCurrentEnergyMode(),
          getFansStatus(),
          getFanProfiles(),
          getEnergySchedule()
        ]);

        if (modeRes.data.success) {
          setCurrentMode(modeRes.data.mode);
        }

        if (fansRes.data.success) {
          setFans(fansRes.data.fans);
          setFansAvailable(fansRes.data.available);
        }

        if (profilesRes.data.success) {
          setFanProfiles(profilesRes.data.profiles);
        }

        if (scheduleRes.data.success) {
          setSchedule(scheduleRes.data.config);
        }
      } catch (error) {
        console.error('Error fetching initial data:', error);
      } finally {
        setLoading(false);
      }
    }

    fetchInitialData();
  }, []);

  // CPU and fans polling
  useEffect(() => {
    async function pollData() {
      try {
        const [cpuRes, fansRes] = await Promise.all([
          getCpuInfo(),
          getFansStatus()
        ]);

        if (cpuRes.data.success) {
          setCpuInfo({
            temperature: cpuRes.data.temperature,
            frequency: cpuRes.data.frequency,
            usage: cpuRes.data.usage
          });
        }

        if (fansRes.data.success) {
          setFans(fansRes.data.fans);
        }
      } catch (error) {
        console.error('Error polling data:', error);
      }
    }

    pollData();
    pollingRef.current = setInterval(pollData, 2000);

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

  // Fan curve change handler
  const handleCurveChange = useCallback((profileName, fanId, newCurve) => {
    setFanProfiles(prev => prev.map(p => {
      if (p.name !== profileName) return p;
      return {
        ...p,
        fans: {
          ...p.fans,
          [fanId]: {
            ...p.fans[fanId],
            curve: newCurve
          }
        }
      };
    }));
    setProfilesModified(true);
  }, []);

  // Save fan profiles
  async function handleSaveProfiles() {
    setSavingProfiles(true);
    try {
      // Save all profiles
      for (const profile of fanProfiles) {
        await saveFanProfile(profile);
      }
      setProfilesModified(false);
    } catch (error) {
      console.error('Error saving profiles:', error);
    } finally {
      setSavingProfiles(false);
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
    <div className="space-y-6">
      <h1 className="text-2xl font-bold flex items-center gap-2">
        <Zap className="text-yellow-400" />
        Énergie
      </h1>

      {/* CPU Info Card */}
      <Card title="Infos CPU (Ryzen 9 3900X)" icon={Cpu}>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
          {/* Temperature */}
          <div className="bg-gray-900 rounded-lg p-4">
            <div className="flex items-center gap-2 text-gray-400 mb-2">
              <Thermometer size={16} />
              <span className="text-sm">Température</span>
            </div>
            <div className={`text-3xl font-bold ${getTempColor(cpuInfo.temperature || 0)}`}>
              {cpuInfo.temperature !== null ? `${cpuInfo.temperature.toFixed(0)}°C` : '--'}
            </div>
            <div className="mt-2 h-2 bg-gray-700 rounded-full overflow-hidden">
              <div
                className={`h-full ${getTempBarColor(cpuInfo.temperature || 0)} transition-all`}
                style={{ width: `${Math.min(100, ((cpuInfo.temperature || 0) / 95) * 100)}%` }}
              />
            </div>
            <div className="text-xs text-gray-500 mt-1">max 95°C</div>
          </div>

          {/* Frequency */}
          <div className="bg-gray-900 rounded-lg p-4">
            <div className="flex items-center gap-2 text-gray-400 mb-2">
              <Zap size={16} />
              <span className="text-sm">Fréquence</span>
            </div>
            <div className="text-3xl font-bold text-blue-400">
              {cpuInfo.frequency?.current != null
                ? `${cpuInfo.frequency.current.toFixed(1)} GHz`
                : '--'}
            </div>
            <div className="text-sm text-gray-500 mt-2">
              {cpuInfo.frequency?.min && cpuInfo.frequency?.max
                ? `${cpuInfo.frequency.min.toFixed(1)} - ${cpuInfo.frequency.max.toFixed(1)} GHz`
                : '--'}
            </div>
          </div>

          {/* Usage */}
          <div className="bg-gray-900 rounded-lg p-4">
            <div className="flex items-center gap-2 text-gray-400 mb-2">
              <Cpu size={16} />
              <span className="text-sm">Usage CPU</span>
            </div>
            <div className="text-3xl font-bold text-purple-400">
              {cpuInfo.usage !== null ? `${cpuInfo.usage.toFixed(0)}%` : '--'}
            </div>
            <div className="mt-2 h-2 bg-gray-700 rounded-full overflow-hidden">
              <div
                className="h-full bg-purple-500 transition-all"
                style={{ width: `${cpuInfo.usage || 0}%` }}
              />
            </div>
          </div>
        </div>
      </Card>

      {/* Mode Card */}
      <Card title="Mode" icon={Zap}>
        <div className="space-y-4">
          {/* Mode buttons */}
          <div className="flex flex-wrap gap-4 justify-center md:justify-start">
            {['economy', 'auto', 'performance'].map(mode => {
              const Icon = MODE_ICONS[mode];
              const isActive = currentMode === mode;
              const colors = MODE_COLORS[mode];

              return (
                <button
                  key={mode}
                  onClick={() => handleModeChange(mode)}
                  disabled={changingMode}
                  className={`flex flex-col items-center justify-center w-28 h-28 rounded-xl transition-all ${
                    isActive
                      ? `${colors.bg} text-white ring-2 ${colors.ring} shadow-lg`
                      : `bg-gray-800 text-gray-300 ${colors.hover}`
                  } ${changingMode ? 'opacity-50 cursor-wait' : ''}`}
                >
                  <Icon size={32} className={isActive ? 'text-white' : 'text-gray-400'} />
                  <span className="mt-2 font-medium">{MODE_LABELS[mode]}</span>
                  {isActive && (
                    <span className="text-xs opacity-75 mt-1">actif</span>
                  )}
                </button>
              );
            })}
          </div>

          {/* Mode description and fan status */}
          <div className="bg-gray-900 rounded-lg p-4">
            <p className="text-gray-300">{MODE_DESCRIPTIONS[currentMode]}</p>
            {fansAvailable && fans.length > 0 && (
              <div className="mt-3 flex flex-wrap gap-4 text-sm text-gray-400">
                {fans.map(fan => (
                  <div key={fan.id} className="flex items-center gap-2">
                    <Fan size={14} className={fan.rpm > 0 ? 'text-blue-400' : 'text-gray-500'} />
                    <span>{fan.name}: <span className="text-white font-mono">{fan.rpm}</span> RPM</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </Card>

      {/* Fan Curves Card */}
      {fansAvailable && (
        <Card title="Courbes ventilateurs" icon={Fan}>
          <div className="space-y-4">
            {/* Profile tabs */}
            <div className="flex gap-2">
              {fanProfiles.map(profile => (
                <button
                  key={profile.name}
                  onClick={() => setEditingProfile(profile.name)}
                  className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                    editingProfile === profile.name
                      ? 'bg-blue-600 text-white'
                      : 'bg-gray-700 text-gray-300 hover:bg-gray-600'
                  }`}
                >
                  {profile.label}
                </button>
              ))}
            </div>

            {/* Curve editor */}
            <FanCurveEditor
              profiles={fanProfiles}
              activeProfile={editingProfile}
              onCurveChange={handleCurveChange}
              onSave={handleSaveProfiles}
              saving={savingProfiles}
            />

            {profilesModified && !savingProfiles && (
              <p className="text-yellow-400 text-sm">
                Modifications non enregistrées
              </p>
            )}
          </div>
        </Card>
      )}

      {/* Schedule Card */}
      <Card title="Programmation automatique" icon={Clock}>
        <div className="space-y-4">
          {/* Enable toggle */}
          <div className="flex items-center gap-3">
            <button
              onClick={() => setSchedule(prev => ({ ...prev, enabled: !prev.enabled }))}
              className={`relative w-12 h-6 rounded-full transition-colors ${
                schedule.enabled ? 'bg-blue-600' : 'bg-gray-600'
              }`}
            >
              <span
                className={`absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full transition-transform ${
                  schedule.enabled ? 'translate-x-6' : ''
                }`}
              />
            </button>
            <span className="text-gray-300">Activer la programmation</span>
          </div>

          {schedule.enabled && (
            <div className="bg-gray-900 rounded-lg p-4 space-y-4">
              {/* Time settings */}
              <div className="flex flex-wrap items-center gap-3">
                <span className="text-gray-400">Mode nuit de</span>
                <input
                  type="time"
                  value={schedule.nightStart}
                  onChange={e => setSchedule(prev => ({ ...prev, nightStart: e.target.value }))}
                  className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-white"
                />
                <span className="text-gray-400">à</span>
                <input
                  type="time"
                  value={schedule.nightEnd}
                  onChange={e => setSchedule(prev => ({ ...prev, nightEnd: e.target.value }))}
                  className="bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-white"
                />
              </div>

              {/* Mode settings */}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                  <label className="block text-sm text-gray-400 mb-1">Mode jour</label>
                  <select
                    value={schedule.dayMode}
                    onChange={e => setSchedule(prev => ({ ...prev, dayMode: e.target.value }))}
                    className="w-full bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-white"
                  >
                    <option value="economy">Économie</option>
                    <option value="auto">Auto</option>
                    <option value="performance">Performance</option>
                  </select>
                </div>
                <div>
                  <label className="block text-sm text-gray-400 mb-1">Mode nuit</label>
                  <select
                    value={schedule.nightMode}
                    onChange={e => setSchedule(prev => ({ ...prev, nightMode: e.target.value }))}
                    className="w-full bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-white"
                  >
                    <option value="economy">Économie</option>
                    <option value="auto">Auto</option>
                    <option value="performance">Performance</option>
                  </select>
                </div>
              </div>

              {/* Save button */}
              <div className="flex items-center gap-3">
                <Button onClick={handleSaveSchedule} loading={savingSchedule}>
                  Enregistrer
                </Button>
                {scheduleMessage && (
                  <span className={scheduleMessage.type === 'success' ? 'text-green-400' : 'text-red-400'}>
                    {scheduleMessage.text}
                  </span>
                )}
              </div>
            </div>
          )}
        </div>
      </Card>
    </div>
  );
}

export default Energy;
