import { useEffect, useState, useRef, useCallback } from 'react';
import { getHosts, wakeHost } from '../api/client';
import useWebSocket from './useWebSocket';

const CLOUDMASTER_ID = '9396e073-9069-482b-bde3-41c8aab3d1ba';
const CLOUDMASTER_NAME = 'CloudMaster';

function findCloudMaster(list) {
  if (!Array.isArray(list)) return null;
  return list.find(h => h.id === CLOUDMASTER_ID)
    || list.find(h => h.name === CLOUDMASTER_NAME)
    || null;
}

function deriveStatus(host) {
  if (!host) return 'offline';
  if (host.power_state && host.power_state !== 'online' && host.power_state !== 'offline') {
    return host.power_state;
  }
  return host.status || 'offline';
}

/**
 * Tracks CloudMaster (10.0.0.10) power status in real time.
 *
 * Returns:
 *   - status: 'loading' | 'online' | 'offline' | 'waking_up' | 'shutting_down' | 'rebooting'
 *   - hostId: CloudMaster id (string)
 *   - wake: () => Promise — sends WOL and optimistically sets status to 'waking_up'
 */
export default function useCloudMasterStatus() {
  const [status, setStatus] = useState('loading');
  const [hostId, setHostId] = useState(CLOUDMASTER_ID);
  const allowFakeOffline = useRef(false);

  if (typeof window !== 'undefined' && !allowFakeOffline.current) {
    allowFakeOffline.current = new URLSearchParams(window.location.search).get('fakeOffline') === '1';
  }

  useEffect(() => {
    if (allowFakeOffline.current) {
      setStatus('offline');
      return;
    }
    let cancelled = false;
    getHosts()
      .then(res => {
        if (cancelled) return;
        const list = res?.data?.hosts || [];
        const cm = findCloudMaster(list);
        if (cm) {
          setHostId(cm.id);
          setStatus(deriveStatus(cm));
        } else {
          setStatus('offline');
        }
      })
      .catch(() => {
        if (!cancelled) setStatus('offline');
      });
    return () => { cancelled = true; };
  }, []);

  useWebSocket({
    'hosts:power': (data) => {
      if (allowFakeOffline.current) return;
      if (!data || data.hostId !== hostId) return;
      setStatus(prev => {
        if (prev !== data.state) {
          console.info('[useCloudMasterStatus] %s -> %s', prev, data.state);
        }
        return data.state || prev;
      });
    },
  });

  const wake = useCallback(async () => {
    if (!hostId) return;
    setStatus(prev => (prev === 'online' ? prev : 'waking_up'));
    try {
      await wakeHost(hostId);
    } catch (err) {
      console.error('[useCloudMasterStatus] wake failed', err);
    }
  }, [hostId]);

  return { status, hostId, wake };
}
