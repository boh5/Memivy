import { useEffect, useRef, useState } from "react";
import { uid } from "./api";

type Message = { id: string; text: string; label?: string; action?: () => void; duration?: number };
const listeners = new Set<(value: Message) => void>();
export function notify(text: string, label?: string, action?: () => void, duration = 4000) {
  const value = { id: uid(), text, label, action, duration };
  listeners.forEach(listener => listener(value));
}
export default function Toast() {
  const [message, setMessage] = useState<Message | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const interaction = useRef({ hover: false, focus: false });
  function pause() { if (timer.current) clearTimeout(timer.current); }
  function resume() { pause(); if (message && !interaction.current.hover && !interaction.current.focus) timer.current = setTimeout(() => setMessage(v => v?.id === message.id ? null : v), message.duration); }
  useEffect(() => { listeners.add(setMessage); return () => { listeners.delete(setMessage); }; }, []);
  useEffect(() => { resume(); return pause; }, [message?.id]);
  return <div className="workspace-toast-region" aria-live="polite" aria-atomic="true">
    {message && <div className="workspace-toast"
      onMouseEnter={() => { interaction.current.hover = true; pause(); }}
      onMouseLeave={() => { interaction.current.hover = false; resume(); }}
      onFocus={() => { interaction.current.focus = true; pause(); }}
      onBlur={e => { if (!e.currentTarget.contains(e.relatedTarget)) { interaction.current.focus = false; resume(); } }}>
      <span>{message.text}</span>
      {message.action && <button onClick={() => { interaction.current = { hover: false, focus: false }; setMessage(null); message.action?.(); }}>{message.label}</button>}
      <button aria-label="关闭提示" onClick={() => { interaction.current = { hover: false, focus: false }; setMessage(null); }}>×</button>
    </div>}
  </div>;
}
