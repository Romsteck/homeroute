import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';

export default function TerminalView({ sessionId, ws, isActive }) {
  const containerRef = useRef(null);
  const termRef = useRef(null);
  const fitAddonRef = useRef(null);
  const initedRef = useRef(false);
  const startedRef = useRef(false);
  const lastDimsRef = useRef({ cols: 0, rows: 0 });
  // Store ws in a ref so the effect doesn't depend on the ws object reference
  const wsRef = useRef(ws);
  wsRef.current = ws;

  // Initialize terminal — depends only on sessionId (stable for CLI tabs)
  useEffect(() => {
    if (!containerRef.current || initedRef.current) return;
    initedRef.current = true;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', Menlo, monospace",
      theme: {
        background: '#111827',
        foreground: '#d1d5db',
        cursor: '#818cf8',
        selectionBackground: '#374151',
        black: '#1f2937',
        red: '#f87171',
        green: '#34d399',
        yellow: '#fbbf24',
        blue: '#60a5fa',
        magenta: '#c084fc',
        cyan: '#22d3ee',
        white: '#e5e7eb',
        brightBlack: '#4b5563',
        brightRed: '#fca5a5',
        brightGreen: '#6ee7b7',
        brightYellow: '#fde68a',
        brightBlue: '#93c5fd',
        brightMagenta: '#d8b4fe',
        brightCyan: '#67e8f9',
        brightWhite: '#f9fafb',
      },
      allowProposedApi: true,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(webLinksAddon);
    term.open(containerRef.current);

    termRef.current = term;
    fitAddonRef.current = fitAddon;

    // Fit and send resize only if dimensions actually changed
    const fitAndNotify = () => {
      if (!fitAddonRef.current || !termRef.current) return;
      fitAddonRef.current.fit();
      const { cols, rows } = termRef.current;
      if (cols === lastDimsRef.current.cols && rows === lastDimsRef.current.rows) return;
      lastDimsRef.current = { cols, rows };
      if (startedRef.current) {
        wsRef.current.sendRaw({ type: 'terminal_resize', session_id: sessionId, cols, rows });
      }
    };

    // Send terminal_start, retrying until WS is connected
    const sendStart = () => {
      if (startedRef.current) return;
      if (!wsRef.current.connected) return false;
      fitAddonRef.current?.fit();
      const cols = termRef.current?.cols || 80;
      const rows = termRef.current?.rows || 24;
      lastDimsRef.current = { cols, rows };
      wsRef.current.sendRaw({ type: 'terminal_start', session_id: sessionId, cols, rows });
      startedRef.current = true;
      term.focus();
      return true;
    };

    // Try to send start immediately, retry every 500ms if WS not connected
    const startTimer = setInterval(() => {
      if (sendStart()) clearInterval(startTimer);
    }, 500);
    // Also try once after initial layout settles
    setTimeout(() => {
      requestAnimationFrame(() => {
        if (sendStart()) clearInterval(startTimer);
      });
    }, 200);

    // Forward user input to backend
    term.onData((input) => {
      wsRef.current.sendRaw({
        type: 'terminal_data',
        session_id: sessionId,
        data: btoa(input),
      });
    });

    // Debounced ResizeObserver
    let resizeTimer = null;
    const resizeObserver = new ResizeObserver(() => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => fitAndNotify(), 150);
    });
    resizeObserver.observe(containerRef.current);

    // Subscribe to terminal output from backend
    const unsubData = wsRef.current.subscribe('terminal_data', (data) => {
      if (data.session_id === sessionId && termRef.current) {
        // Decode base64 to raw bytes (Uint8Array) — atob returns Latin-1, not UTF-8
        const binary = atob(data.data);
        const bytes = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
        termRef.current.write(bytes);
      }
    });

    const unsubDone = wsRef.current.subscribe('terminal_done', (data) => {
      if (data.session_id === sessionId && termRef.current) {
        termRef.current.write('\r\n\x1b[90m[Session ended]\x1b[0m\r\n');
      }
    });

    const unsubError = wsRef.current.subscribe('error', (data) => {
      if (data.session_id === sessionId && termRef.current) {
        termRef.current.write(`\r\n\x1b[31mError: ${data.message}\x1b[0m\r\n`);
      }
    });

    return () => {
      clearInterval(startTimer);
      clearTimeout(resizeTimer);
      resizeObserver.disconnect();
      unsubData();
      unsubDone();
      unsubError();
      term.dispose();
      termRef.current = null;
      fitAddonRef.current = null;
      initedRef.current = false;
      startedRef.current = false;
    };
  }, [sessionId]); // Only sessionId — ws access via wsRef

  // Re-fit when isActive changes (tab becomes visible)
  useEffect(() => {
    if (isActive && fitAddonRef.current && termRef.current) {
      setTimeout(() => {
        requestAnimationFrame(() => {
          if (fitAddonRef.current && termRef.current) {
            fitAddonRef.current.fit();
            termRef.current.focus();
          }
        });
      }, 50);
    }
  }, [isActive]);

  return (
    <div
      ref={containerRef}
      className="flex-1 min-h-0 min-w-0 bg-[#111827] overflow-hidden"
      style={{ padding: '4px' }}
    />
  );
}
