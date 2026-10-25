import { useEffect, type ReactNode } from "react";

interface ModalProps {
  title: string;
  subtitle?: string;
  children: ReactNode;
  footer: ReactNode;
  width?: string;
  onClose: () => void;
}

export default function Modal({ title, subtitle, children, footer, width, onClose }: ModalProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-stone-900/30 p-6">
      <div
        className={
          "flex max-h-full w-full flex-col overflow-hidden rounded-xl bg-white shadow-xl ring-1 ring-stone-900/10 " +
          (width ?? "max-w-lg")
        }
      >
        <div className="border-b border-stone-100 px-6 py-4">
          <h2 className="text-base font-semibold text-stone-800">{title}</h2>
          {subtitle && <p className="mt-1 text-xs text-stone-500">{subtitle}</p>}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">{children}</div>
        <div className="flex items-center justify-end gap-2 border-t border-stone-100 bg-stone-50 px-6 py-3">
          {footer}
        </div>
      </div>
    </div>
  );
}
