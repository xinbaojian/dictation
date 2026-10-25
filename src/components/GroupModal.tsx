import { useState } from "react";
import type { Group } from "../types";
import Modal from "./Modal";

interface GroupModalProps {
  group: Group | null;
  parent: Group | null;
  nextCode: string;
  onSubmit: (code: string, name: string) => Promise<void>;
  onClose: () => void;
}

export default function GroupModal({ group, parent, nextCode, onSubmit, onClose }: GroupModalProps) {
  const [code, setCode] = useState(group ? group.code : nextCode);
  const [name, setName] = useState(group ? group.name : "");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    try {
      await onSubmit(code, name);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      title={group ? "编辑分组" : parent ? "新建子分组" : "新建分组"}
      subtitle={
        parent
          ? "将创建在「" + parent.path + "」下面"
          : "编号可留空，系统自动按顺序编号"
      }
      onClose={onClose}
      footer={
        <>
          <button
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-200/70"
          >
            取消
          </button>
          <button
            onClick={submit}
            disabled={!name.trim() || busy}
            className="rounded-md bg-stone-800 px-4 py-1.5 text-sm font-medium text-white hover:bg-stone-900 disabled:cursor-not-allowed disabled:opacity-40"
          >
            保存
          </button>
        </>
      }
    >
      <div className="space-y-4">
        <label className="block">
          <span className="mb-1.5 block text-xs font-medium text-stone-500">编号</span>
          <input
            value={code}
            onChange={(e) => setCode(e.target.value)}
            autoFocus
            className="w-28 rounded-md border border-stone-200 px-3 py-2 text-sm outline-none focus:border-stone-400"
          />
        </label>
        <label className="block">
          <span className="mb-1.5 block text-xs font-medium text-stone-500">名称</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void submit();
            }}
            placeholder={parent ? "如：第一单元" : "如：二年级上册"}
            className="w-full rounded-md border border-stone-200 px-3 py-2 text-sm outline-none focus:border-stone-400"
          />
        </label>
      </div>
    </Modal>
  );
}
