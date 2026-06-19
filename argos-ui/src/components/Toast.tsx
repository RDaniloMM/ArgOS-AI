import { useEffect } from "react";
import { CheckCircle2, XCircle, X } from "lucide-react";

export interface ToastMessage {
  id: string;
  type: "success" | "error";
  title: string;
  message: string;
}

interface ToastProps {
  toast: ToastMessage;
  onDismiss: (id: string) => void;
}

export function Toast({ toast, onDismiss }: ToastProps) {
  useEffect(() => {
    const timer = setTimeout(() => onDismiss(toast.id), 4500);
    return () => clearTimeout(timer);
  }, [toast.id, onDismiss]);

  const isSuccess = toast.type === "success";

  return (
    <div
      className={[
        "animate-slide-in pointer-events-auto flex w-80 items-start gap-3 rounded-xl p-4 shadow-2xl backdrop-blur",
        isSuccess
          ? "bg-emerald-500/10 ring-1 ring-emerald-500/30"
          : "bg-rose-500/10 ring-1 ring-rose-500/30",
      ].join(" ")}
      role="status"
    >
      {isSuccess ? (
        <CheckCircle2 className="mt-0.5 h-5 w-5 shrink-0 text-emerald-400" />
      ) : (
        <XCircle className="mt-0.5 h-5 w-5 shrink-0 text-rose-400" />
      )}
      <div className="min-w-0 flex-1">
        <p
          className={[
            "text-sm font-semibold",
            isSuccess ? "text-emerald-200" : "text-rose-200",
          ].join(" ")}
        >
          {toast.title}
        </p>
        <p className="mt-0.5 text-sm text-slate-300">{toast.message}</p>
      </div>
      <button
        onClick={() => onDismiss(toast.id)}
        className="ml-1 shrink-0 rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100"
        aria-label="Dismiss"
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}

interface ToastContainerProps {
  toasts: ToastMessage[];
  onDismiss: (id: string) => void;
}

export function ToastContainer({ toasts, onDismiss }: ToastContainerProps) {
  return (
    <div className="fixed bottom-6 right-6 z-50 flex flex-col gap-3">
      {toasts.map((toast) => (
        <Toast key={toast.id} toast={toast} onDismiss={onDismiss} />
      ))}
    </div>
  );
}
