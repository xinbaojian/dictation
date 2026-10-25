import Modal from "./Modal";

interface ConfirmModalProps {
  title: string;
  body: string;
  confirmText?: string;
  cancelText?: string;
  extraText?: string;
  onExtra?: () => void;
  onCancel: () => void;
  onConfirm: () => void;
}

export default function ConfirmModal({
  title,
  body,
  confirmText = "确认删除",
  cancelText = "取消",
  extraText,
  onExtra,
  onCancel,
  onConfirm,
}: ConfirmModalProps) {
  return (
    <Modal
      title={title}
      onClose={onCancel}
      footer={
        <>
          {extraText && onExtra && (
            <button
              onClick={onExtra}
              className="mr-auto rounded-md px-3 py-1.5 text-sm text-stone-400 hover:bg-stone-100 hover:text-stone-600"
            >
              {extraText}
            </button>
          )}
          <button
            onClick={onCancel}
            className="rounded-md px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-200/70"
          >
            {cancelText}
          </button>
          <button
            onClick={onConfirm}
            className="rounded-md bg-stone-800 px-3 py-1.5 text-sm font-medium text-white hover:bg-stone-900"
          >
            {confirmText}
          </button>
        </>
      }
    >
      <p className="text-sm leading-relaxed text-stone-600">{body}</p>
    </Modal>
  );
}
