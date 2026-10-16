import { useCallback, useRef } from "react";

/**
 * Distinguishes single / double / triple clicks on an element.
 *
 * It waits a short window after each click; when the window elapses with no
 * further click, it fires the handler for the accumulated count (capped at 3).
 * This means a single click is delayed by `delayMs` — acceptable for widget
 * actions and necessary to disambiguate from double/triple.
 */
export function useClickCount(
  onClicks: (count: 1 | 2 | 3) => void,
  delayMs = 350
) {
  const count = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  return useCallback(
    (e: React.MouseEvent) => {
      // Ignore non-primary buttons.
      if (e.button !== 0) return;
      count.current += 1;

      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        const n = Math.min(count.current, 3) as 1 | 2 | 3;
        count.current = 0;
        timer.current = null;
        onClicks(n);
      }, delayMs);
    },
    [onClicks, delayMs]
  );
}
