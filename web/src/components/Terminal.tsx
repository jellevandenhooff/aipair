import { useEffect, useRef } from 'react';

interface TerminalProps {
  sessionName: string;
}

export function Terminal({ sessionName }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const cleanupRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let cancelled = false;

    async function setup() {
      const { init, Terminal: GhosttyTerminal, FitAddon } = await import('ghostty-web');
      await init();

      if (cancelled) return;

      const fitAddon = new FitAddon();
      const term = new GhosttyTerminal({
        cursorBlink: true,
        fontSize: 14,
        fontFamily: 'Monaco, Menlo, "Courier New", monospace',
        theme: {
          background: '#1e1e1e',
          foreground: '#d4d4d4',
        },
        scrollback: 10000,
      });

      term.loadAddon(fitAddon);
      term.open(container!);
      fitAddon.fit();

      // Connect WebSocket
      const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      const wsUrl = `${proto}//${window.location.host}/api/sessions/${encodeURIComponent(sessionName)}/terminal?cols=${term.cols}&rows=${term.rows}`;
      const ws = new WebSocket(wsUrl);
      ws.binaryType = 'arraybuffer';

      ws.onopen = () => {
        term.focus();
      };

      ws.onmessage = (event) => {
        if (event.data instanceof ArrayBuffer) {
          term.write(new Uint8Array(event.data));
        } else {
          term.write(event.data);
        }
      };

      ws.onclose = () => {
        term.write('\r\n\x1b[90m[Connection closed]\x1b[0m\r\n');
      };

      // User input → WebSocket (binary)
      term.onData((data: string) => {
        if (ws.readyState === WebSocket.OPEN) {
          const encoder = new TextEncoder();
          ws.send(encoder.encode(data));
        }
      });

      // Resize → WebSocket JSON control message
      term.onResize((size: { cols: number; rows: number }) => {
        if (ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ type: 'resize', cols: size.cols, rows: size.rows }));
        }
      });

      // Observe container resize
      fitAddon.observeResize();

      cleanupRef.current = () => {
        ws.close();
        term.dispose();
      };
    }

    setup();

    return () => {
      cancelled = true;
      cleanupRef.current?.();
      cleanupRef.current = null;
    };
  }, [sessionName]);

  return (
    <div
      ref={containerRef}
      className="flex-1 bg-[#1e1e1e]"
      style={{ minHeight: 0 }}
    />
  );
}
