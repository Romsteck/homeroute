import { useEffect, useRef } from 'react'

type EventHandler = (data: any) => void

/**
 * Hook to connect to the HomeRoute WebSocket and listen for events.
 * Auto-reconnects on disconnect (3s delay).
 */
export function useWebSocket(handlers: Record<string, EventHandler>) {
  const wsRef = useRef<WebSocket | null>(null)
  const handlersRef = useRef(handlers)
  handlersRef.current = handlers

  useEffect(() => {
    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const url = `${proto}//${window.location.host}/api/ws`

    let ws: WebSocket
    let reconnectTimer: ReturnType<typeof setTimeout>

    function connect() {
      ws = new WebSocket(url)
      wsRef.current = ws

      ws.onmessage = (e) => {
        try {
          const msg = JSON.parse(e.data)
          const handler = handlersRef.current[msg.type]
          if (handler) handler(msg.data)
        } catch {
          // ignore parse errors
        }
      }

      ws.onclose = () => {
        reconnectTimer = setTimeout(connect, 3000)
      }

      ws.onerror = () => {
        ws.close()
      }
    }

    connect()

    return () => {
      clearTimeout(reconnectTimer)
      wsRef.current?.close()
    }
  }, [])

  return wsRef
}
