import { useEffect, useState, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";

/** 底部的短暂提示。放在哪个容器里由调用方决定，位置见样式里的 .toast。 */
export function Toast({ message, action }: { message: string | null; action?: ReactNode }) {
  return (
    <AnimatePresence>
      {message && (
        <motion.div
          className="toast"
          role="status"
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: 10 }}
          transition={{ duration: 0.2 }}
        >
          <span>{message}</span>
          {action}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** 提示文字和它的自动消失。 */
export function useToast(duration = 3200) {
  const [message, setMessage] = useState<string | null>(null);
  useEffect(() => {
    if (!message) return;
    const timer = window.setTimeout(() => setMessage(null), duration);
    return () => window.clearTimeout(timer);
  }, [message, duration]);
  return [message, setMessage] as const;
}
