import { readFile, writeFile, mkdir, readdir } from 'fs/promises';
import { existsSync } from 'fs';
import { exec } from 'child_process';
import { promisify } from 'util';
import path from 'path';

const execAsync = promisify(exec);

// Config paths
const CONFIG_DIR = process.env.ENERGY_CONFIG_DIR || '/var/lib/server-dashboard';
const SCHEDULE_CONFIG_FILE = path.join(CONFIG_DIR, 'energy-schedule.json');
const FAN_PROFILES_FILE = path.join(CONFIG_DIR, 'fan-profiles.json');

// Unified energy modes
export const ENERGY_MODES = {
  economy: { governor: 'powersave', fanProfile: 'silent', label: 'Économie', icon: 'Moon' },
  auto: { governor: 'schedutil', fanProfile: 'balanced', label: 'Auto', icon: 'Zap' },
  performance: { governor: 'performance', fanProfile: 'performance', label: 'Performance', icon: 'Rocket' }
};

// CPU sysfs paths
const CPU_FREQ_PATH = '/sys/devices/system/cpu/cpu0/cpufreq';
const HWMON_PATH = '/sys/class/hwmon';

// Cache for hwmon paths (they can change between boots)
let it87HwmonPath = null;
let k10tempHwmonPath = null;

// Previous CPU stats for usage calculation
let prevCpuStats = null;

async function ensureConfigDir() {
  if (!existsSync(CONFIG_DIR)) {
    await mkdir(CONFIG_DIR, { recursive: true });
  }
}

// Find hwmon device by name
async function findHwmonByName(name) {
  try {
    const hwmons = await readdir(HWMON_PATH);
    for (const hwmon of hwmons) {
      const namePath = path.join(HWMON_PATH, hwmon, 'name');
      if (existsSync(namePath)) {
        const hwmonName = (await readFile(namePath, 'utf-8')).trim();
        if (hwmonName === name) {
          return path.join(HWMON_PATH, hwmon);
        }
      }
    }
  } catch {
    // Ignore errors
  }
  return null;
}

// Get IT87 hwmon path (cached)
async function getIt87Path() {
  if (!it87HwmonPath) {
    it87HwmonPath = await findHwmonByName('it8686');
    if (!it87HwmonPath) {
      // Try finding by platform device
      try {
        const { stdout } = await execAsync('ls -d /sys/devices/platform/it87.*/hwmon/hwmon* 2>/dev/null | head -1');
        it87HwmonPath = stdout.trim() || null;
      } catch {
        it87HwmonPath = null;
      }
    }
  }
  return it87HwmonPath;
}

// Get k10temp hwmon path (cached)
async function getK10tempPath() {
  if (!k10tempHwmonPath) {
    k10tempHwmonPath = await findHwmonByName('k10temp');
  }
  return k10tempHwmonPath;
}

// Read a sysfs file safely
async function readSysfs(filePath) {
  try {
    const content = await readFile(filePath, 'utf-8');
    return content.trim();
  } catch {
    return null;
  }
}

// Write to a sysfs file
async function writeSysfs(filePath, value) {
  try {
    await writeFile(filePath, String(value));
    return true;
  } catch {
    return false;
  }
}

// ============ CPU INFO ============

export async function getCpuTemperature() {
  try {
    const k10tempPath = await getK10tempPath();
    if (!k10tempPath) {
      return { success: false, error: 'k10temp not found' };
    }

    const tempRaw = await readSysfs(path.join(k10tempPath, 'temp1_input'));
    if (!tempRaw) {
      return { success: false, error: 'Cannot read temperature' };
    }

    const tempC = parseInt(tempRaw) / 1000;
    return { success: true, temperature: tempC };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function getCpuFrequency() {
  try {
    const currentRaw = await readSysfs(path.join(CPU_FREQ_PATH, 'scaling_cur_freq'));
    const minRaw = await readSysfs(path.join(CPU_FREQ_PATH, 'scaling_min_freq'));
    const maxRaw = await readSysfs(path.join(CPU_FREQ_PATH, 'scaling_max_freq'));

    return {
      success: true,
      current: currentRaw ? parseInt(currentRaw) / 1000000 : null, // GHz
      min: minRaw ? parseInt(minRaw) / 1000000 : null,
      max: maxRaw ? parseInt(maxRaw) / 1000000 : null
    };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function getCpuUsage() {
  try {
    const statContent = await readFile('/proc/stat', 'utf-8');
    const cpuLine = statContent.split('\n').find(line => line.startsWith('cpu '));
    if (!cpuLine) {
      return { success: false, error: 'Cannot read CPU stats' };
    }

    const parts = cpuLine.split(/\s+/).slice(1).map(Number);
    const [user, nice, system, idle, iowait, irq, softirq, steal] = parts;

    const total = user + nice + system + idle + iowait + irq + softirq + steal;
    const idleTime = idle + iowait;

    if (!prevCpuStats) {
      prevCpuStats = { total, idle: idleTime };
      return { success: true, usage: 0 };
    }

    const totalDiff = total - prevCpuStats.total;
    const idleDiff = idleTime - prevCpuStats.idle;

    prevCpuStats = { total, idle: idleTime };

    const usage = totalDiff > 0 ? ((totalDiff - idleDiff) / totalDiff) * 100 : 0;
    return { success: true, usage: Math.round(usage * 10) / 10 };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function getCpuInfo() {
  const [tempResult, freqResult, usageResult] = await Promise.all([
    getCpuTemperature(),
    getCpuFrequency(),
    getCpuUsage()
  ]);

  return {
    success: true,
    temperature: tempResult.success ? tempResult.temperature : null,
    frequency: freqResult.success ? freqResult : null,
    usage: usageResult.success ? usageResult.usage : null
  };
}

// ============ GOVERNOR ============

export async function getCurrentGovernor() {
  try {
    const governor = await readSysfs(path.join(CPU_FREQ_PATH, 'scaling_governor'));
    return { success: true, governor };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function getAvailableGovernors() {
  try {
    const governors = await readSysfs(path.join(CPU_FREQ_PATH, 'scaling_available_governors'));
    return { success: true, governors: governors ? governors.split(' ') : [] };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function setGovernor(governor) {
  try {
    // Validate governor
    const available = await getAvailableGovernors();
    if (!available.success || !available.governors.includes(governor)) {
      return { success: false, error: `Invalid governor: ${governor}` };
    }

    // Set governor on all CPUs
    const { stdout } = await execAsync('ls /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor');
    const files = stdout.trim().split('\n');

    for (const file of files) {
      await writeSysfs(file, governor);
    }

    return { success: true, message: `Governor set to ${governor}` };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function getGovernorStatus() {
  const [current, available] = await Promise.all([
    getCurrentGovernor(),
    getAvailableGovernors()
  ]);

  return {
    success: true,
    current: current.success ? current.governor : null,
    available: available.success ? available.governors : []
  };
}

// Get current energy mode based on governor
export async function getCurrentMode() {
  try {
    const { governor } = await getCurrentGovernor();

    // Find which mode matches the current governor
    for (const [modeName, modeConfig] of Object.entries(ENERGY_MODES)) {
      if (modeConfig.governor === governor) {
        return { success: true, mode: modeName, config: modeConfig };
      }
    }

    // Default to auto if governor doesn't match any mode
    return { success: true, mode: 'auto', config: ENERGY_MODES.auto };
  } catch (error) {
    return { success: false, error: error.message, mode: 'auto', config: ENERGY_MODES.auto };
  }
}

// Get all available energy modes
export function getEnergyModes() {
  return { success: true, modes: ENERGY_MODES };
}

// ============ FANS ============

export async function getFanStatus() {
  try {
    const it87Path = await getIt87Path();
    if (!it87Path) {
      return { success: false, error: 'IT87 driver not loaded', available: false };
    }

    const fans = [];

    // Check fan1 and fan2
    for (let i = 1; i <= 2; i++) {
      const rpmPath = path.join(it87Path, `fan${i}_input`);
      const pwmPath = path.join(it87Path, `pwm${i}`);
      const enablePath = path.join(it87Path, `pwm${i}_enable`);

      if (existsSync(rpmPath)) {
        const rpm = await readSysfs(rpmPath);
        const pwm = await readSysfs(pwmPath);
        const enable = await readSysfs(enablePath);

        fans.push({
          id: `fan${i}`,
          name: i === 1 ? 'CPU_FAN' : 'SYS_FAN',
          rpm: rpm ? parseInt(rpm) : 0,
          pwm: pwm ? parseInt(pwm) : 0,
          pwmPercent: pwm ? Math.round((parseInt(pwm) / 255) * 100) : 0,
          mode: enable === '1' ? 'manual' : (enable === '2' ? 'auto' : 'off')
        });
      }
    }

    return { success: true, fans, available: true };
  } catch (error) {
    return { success: false, error: error.message, available: false };
  }
}

export async function setFanSpeed(fanId, pwm, mode) {
  try {
    const it87Path = await getIt87Path();
    if (!it87Path) {
      return { success: false, error: 'IT87 driver not loaded' };
    }

    const fanNum = fanId.replace('fan', '');
    const pwmPath = path.join(it87Path, `pwm${fanNum}`);
    const enablePath = path.join(it87Path, `pwm${fanNum}_enable`);

    if (!existsSync(pwmPath)) {
      return { success: false, error: `Fan ${fanId} not found` };
    }

    // Set mode if provided
    if (mode !== undefined) {
      const modeValue = mode === 'manual' ? '1' : (mode === 'auto' ? '2' : '0');
      await writeSysfs(enablePath, modeValue);
    }

    // Set PWM if provided and mode is manual
    if (pwm !== undefined) {
      // Ensure mode is manual before setting PWM
      const currentMode = await readSysfs(enablePath);
      if (currentMode !== '1') {
        await writeSysfs(enablePath, '1');
      }

      const pwmValue = Math.min(255, Math.max(0, Math.round(pwm)));
      await writeSysfs(pwmPath, String(pwmValue));
    }

    return { success: true, message: `Fan ${fanId} updated` };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

// ============ FAN PROFILES ============

const DEFAULT_PROFILES = [
  {
    name: 'silent',
    label: 'Économie',
    fans: {
      fan1: {
        mode: 'manual',
        pwm: 70,
        curve: [[30, 20], [50, 30], [70, 50], [85, 100]]  // [temp°C, pwm%]
      },
      fan2: {
        mode: 'manual',
        pwm: 50,
        curve: [[30, 15], [50, 25], [70, 40], [85, 80]]
      }
    }
  },
  {
    name: 'balanced',
    label: 'Auto',
    fans: {
      fan1: {
        mode: 'auto',
        curve: [[30, 30], [50, 45], [70, 70], [85, 100]]
      },
      fan2: {
        mode: 'auto',
        curve: [[30, 25], [50, 40], [70, 60], [85, 90]]
      }
    }
  },
  {
    name: 'performance',
    label: 'Performance',
    fans: {
      fan1: {
        mode: 'manual',
        pwm: 180,
        curve: [[30, 50], [50, 65], [70, 85], [85, 100]]
      },
      fan2: {
        mode: 'manual',
        pwm: 150,
        curve: [[30, 40], [50, 55], [70, 75], [85, 100]]
      }
    }
  }
];

export async function getFanProfiles() {
  try {
    await ensureConfigDir();

    if (!existsSync(FAN_PROFILES_FILE)) {
      // Return default profiles
      return { success: true, profiles: DEFAULT_PROFILES };
    }

    const content = await readFile(FAN_PROFILES_FILE, 'utf-8');
    const profiles = JSON.parse(content);
    return { success: true, profiles };
  } catch (error) {
    return { success: false, error: error.message, profiles: DEFAULT_PROFILES };
  }
}

export async function saveFanProfile(profile) {
  try {
    await ensureConfigDir();

    let profiles = [];
    if (existsSync(FAN_PROFILES_FILE)) {
      const content = await readFile(FAN_PROFILES_FILE, 'utf-8');
      profiles = JSON.parse(content);
    } else {
      profiles = [...DEFAULT_PROFILES];
    }

    // Update or add profile
    const existingIndex = profiles.findIndex(p => p.name === profile.name);
    if (existingIndex >= 0) {
      profiles[existingIndex] = profile;
    } else {
      profiles.push(profile);
    }

    await writeFile(FAN_PROFILES_FILE, JSON.stringify(profiles, null, 2));
    return { success: true, message: 'Profile saved' };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

export async function applyFanProfile(profileName) {
  try {
    const { profiles } = await getFanProfiles();
    const profile = profiles.find(p => p.name === profileName);

    if (!profile) {
      return { success: false, error: `Profile ${profileName} not found` };
    }

    // Apply each fan setting
    for (const [fanId, settings] of Object.entries(profile.fans)) {
      await setFanSpeed(fanId, settings.pwm, settings.mode);
    }

    return { success: true, message: `Profile ${profileName} applied` };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

// ============ SCHEDULE ============

const DEFAULT_SCHEDULE = {
  enabled: false,
  nightStart: '22:00',
  nightEnd: '08:00',
  dayMode: 'auto',      // economy | auto | performance
  nightMode: 'economy'
};

export async function getScheduleConfig() {
  try {
    await ensureConfigDir();

    if (!existsSync(SCHEDULE_CONFIG_FILE)) {
      return { success: true, config: DEFAULT_SCHEDULE };
    }

    const content = await readFile(SCHEDULE_CONFIG_FILE, 'utf-8');
    const config = JSON.parse(content);
    return { success: true, config: { ...DEFAULT_SCHEDULE, ...config } };
  } catch (error) {
    return { success: false, error: error.message, config: DEFAULT_SCHEDULE };
  }
}

export async function saveScheduleConfig(config) {
  try {
    await ensureConfigDir();

    const newConfig = { ...DEFAULT_SCHEDULE, ...config };
    await writeFile(SCHEDULE_CONFIG_FILE, JSON.stringify(newConfig, null, 2));

    // Sync cron jobs
    await syncCronJobs(newConfig);

    return { success: true, message: 'Schedule saved' };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

// ============ CRON MANAGEMENT ============

const CRON_MARKER = '# server-dashboard-energy';

export async function syncCronJobs(config) {
  try {
    // Read current crontab
    let crontab = '';
    try {
      const { stdout } = await execAsync('crontab -l 2>/dev/null');
      crontab = stdout;
    } catch {
      // No crontab exists
    }

    // Remove existing energy cron jobs
    const lines = crontab.split('\n').filter(line => !line.includes(CRON_MARKER));

    if (config.enabled) {
      // Parse times
      const [nightHour, nightMin] = config.nightStart.split(':').map(Number);
      const [dayHour, dayMin] = config.nightEnd.split(':').map(Number);

      // Add new cron jobs
      const apiUrl = process.env.API_URL || 'http://localhost:4000';

      lines.push(`${nightMin} ${nightHour} * * * curl -X POST ${apiUrl}/api/energy/mode/night ${CRON_MARKER}`);
      lines.push(`${dayMin} ${dayHour} * * * curl -X POST ${apiUrl}/api/energy/mode/day ${CRON_MARKER}`);
    }

    // Write new crontab
    const newCrontab = lines.filter(l => l.trim()).join('\n') + '\n';
    await execAsync(`echo '${newCrontab}' | crontab -`);

    return { success: true };
  } catch (error) {
    console.error('Failed to sync cron jobs:', error);
    return { success: false, error: error.message };
  }
}

// Apply energy mode (economy/auto/performance) or scheduled mode (day/night)
export async function applyMode(mode) {
  try {
    // Handle scheduled modes (day/night)
    if (mode === 'day' || mode === 'night') {
      const { config } = await getScheduleConfig();
      const targetMode = mode === 'night' ? config.nightMode : config.dayMode;
      return applyMode(targetMode);
    }

    // Handle unified energy modes
    if (!ENERGY_MODES[mode]) {
      return { success: false, error: `Invalid mode: ${mode}. Valid modes: economy, auto, performance` };
    }

    const modeConfig = ENERGY_MODES[mode];

    // Apply governor
    await setGovernor(modeConfig.governor);

    // Apply fan profile
    await applyFanProfile(modeConfig.fanProfile);

    return { success: true, message: `Mode ${modeConfig.label} appliqué`, mode, config: modeConfig };
  } catch (error) {
    return { success: false, error: error.message };
  }
}
