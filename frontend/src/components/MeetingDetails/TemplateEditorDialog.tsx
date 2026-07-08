"use client";

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Pencil, Plus, Trash2, Loader2, X } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from '@/components/ui/dialog';
import { VisuallyHidden } from '@/components/ui/visually-hidden';

interface TemplateSummary {
  id: string;
  name: string;
  description: string;
}

interface TemplateSection {
  title: string;
  instruction: string;
  format: 'paragraph' | 'list' | 'string';
  item_format?: string;
}

interface TemplateDetail {
  name: string;
  description: string;
  sections: TemplateSection[];
}

const EMPTY_SECTION: TemplateSection = { title: '', instruction: '', format: 'paragraph' };

const EMPTY_TEMPLATE: TemplateDetail = {
  name: '',
  description: '',
  sections: [{ ...EMPTY_SECTION }],
};

function slugify(name: string): string {
  return (
    name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '_')
      .replace(/^_+|_+$/g, '') || 'custom_template'
  );
}

/**
 * Custom summary template editor (PROJECT_BRIEF.md §13: "you get full
 * prompt-editing access to your own templates by default"). Lists existing
 * templates (built-in ones are read-only sources for duplication; only
 * custom ones can be edited/deleted here), and edits sections directly
 * against the same schema the built-in templates use.
 */
export function TemplateEditorDialog({ onTemplatesChanged }: { onTemplatesChanged?: () => void }) {
  const [open, setOpen] = useState(false);
  const [templates, setTemplates] = useState<TemplateSummary[]>([]);
  const [isLoadingList, setIsLoadingList] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<TemplateDetail>(EMPTY_TEMPLATE);
  const [isSaving, setIsSaving] = useState(false);
  const [validationError, setValidationError] = useState<string | null>(null);

  const loadList = async () => {
    setIsLoadingList(true);
    try {
      setTemplates(await invoke<TemplateSummary[]>('api_list_templates'));
    } catch (error) {
      console.error('Failed to list templates:', error);
      toast.error('Failed to load templates');
    } finally {
      setIsLoadingList(false);
    }
  };

  useEffect(() => {
    if (open) loadList();
  }, [open]);

  const startNew = () => {
    setEditingId(null);
    setDraft({ ...EMPTY_TEMPLATE, sections: [{ ...EMPTY_SECTION }] });
    setValidationError(null);
  };

  const startEdit = async (id: string) => {
    try {
      const full = await invoke<TemplateDetail>('api_get_template_for_edit', { templateId: id });
      setEditingId(id);
      setDraft(full);
      setValidationError(null);
    } catch (error) {
      console.error('Failed to load template for editing:', error);
      toast.error('Failed to load template');
    }
  };

  const startDuplicate = async (id: string) => {
    try {
      const full = await invoke<TemplateDetail>('api_get_template_for_edit', { templateId: id });
      setEditingId(null); // duplicating always creates a new custom template
      setDraft({ ...full, name: `${full.name} (copy)` });
      setValidationError(null);
    } catch (error) {
      console.error('Failed to duplicate template:', error);
      toast.error('Failed to duplicate template');
    }
  };

  const updateSection = (index: number, patch: Partial<TemplateSection>) => {
    setDraft((prev) => ({
      ...prev,
      sections: prev.sections.map((s, i) => (i === index ? { ...s, ...patch } : s)),
    }));
  };

  const addSection = () => {
    setDraft((prev) => ({ ...prev, sections: [...prev.sections, { ...EMPTY_SECTION }] }));
  };

  const removeSection = (index: number) => {
    setDraft((prev) => ({ ...prev, sections: prev.sections.filter((_, i) => i !== index) }));
  };

  const save = async () => {
    setValidationError(null);
    const id = editingId ?? slugify(draft.name);
    const json = JSON.stringify(draft);

    try {
      await invoke<string>('api_validate_template', { templateJson: json });
    } catch (error) {
      setValidationError(String(error));
      return;
    }

    setIsSaving(true);
    try {
      await invoke('api_save_custom_template', { templateId: id, templateJson: json });
      toast.success(`Template "${draft.name}" saved`);
      setEditingId(id);
      await loadList();
      onTemplatesChanged?.();
    } catch (error) {
      console.error('Failed to save template:', error);
      toast.error(String(error));
    } finally {
      setIsSaving(false);
    }
  };

  const remove = async (id: string) => {
    try {
      const removed = await invoke<boolean>('api_delete_custom_template', { templateId: id });
      if (!removed) {
        toast.error("Can't delete a built-in template — only custom ones can be removed here");
        return;
      }
      toast.success('Template deleted');
      if (editingId === id) startNew();
      await loadList();
      onTemplatesChanged?.();
    } catch (error) {
      console.error('Failed to delete template:', error);
      toast.error('Failed to delete template');
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <button
        onClick={() => setOpen(true)}
        className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full border border-gray-200 text-xs font-medium text-gray-600 hover:bg-gray-50"
        title="Create or edit summary templates"
      >
        <Pencil className="w-3.5 h-3.5" />
        Templates
      </button>

      <DialogContent className="max-w-3xl max-h-[85vh] overflow-hidden flex flex-col">
        <VisuallyHidden>
          <DialogTitle>Summary Templates</DialogTitle>
        </VisuallyHidden>

        <div className="flex gap-4 min-h-0 flex-1">
          {/* Template list */}
          <div className="w-52 shrink-0 border-r border-gray-100 pr-3 overflow-y-auto">
            <button
              onClick={startNew}
              className="flex items-center gap-1.5 w-full px-2 py-1.5 mb-2 text-xs font-medium text-blue-600 hover:bg-blue-50 rounded-md"
            >
              <Plus className="w-3.5 h-3.5" /> New template
            </button>
            {isLoadingList ? (
              <div className="text-xs text-gray-400 px-2">Loading…</div>
            ) : (
              templates.map((t) => (
                <div
                  key={t.id}
                  className={`group flex items-center gap-1 px-2 py-1.5 rounded-md text-xs cursor-pointer ${
                    editingId === t.id ? 'bg-gray-100 font-medium' : 'hover:bg-gray-50'
                  }`}
                  onClick={() => startEdit(t.id)}
                  title={t.description}
                >
                  <span className="flex-1 truncate">{t.name}</span>
                  <button
                    onClick={(e) => { e.stopPropagation(); startDuplicate(t.id); }}
                    className="opacity-0 group-hover:opacity-100 text-gray-400 hover:text-blue-600"
                    title="Duplicate as a new custom template"
                  >
                    <Plus className="w-3 h-3" />
                  </button>
                </div>
              ))
            )}
          </div>

          {/* Editor */}
          <div className="flex-1 overflow-y-auto pr-1">
            <div className="flex items-center justify-between mb-3">
              <h2 className="text-sm font-semibold text-gray-800">
                {editingId ? 'Edit Template' : 'New Template'}
              </h2>
              {editingId && (
                <button
                  onClick={() => remove(editingId)}
                  className="flex items-center gap-1 text-xs text-red-500 hover:text-red-700"
                >
                  <Trash2 className="w-3.5 h-3.5" /> Delete
                </button>
              )}
            </div>

            <div className="space-y-3 mb-4">
              <input
                value={draft.name}
                onChange={(e) => setDraft((p) => ({ ...p, name: e.target.value }))}
                placeholder="Template name"
                className="w-full px-2 py-1.5 text-sm border border-gray-200 rounded-md focus:outline-none focus:ring-1 focus:ring-blue-400"
              />
              <input
                value={draft.description}
                onChange={(e) => setDraft((p) => ({ ...p, description: e.target.value }))}
                placeholder="Short description shown in the template picker"
                className="w-full px-2 py-1.5 text-sm border border-gray-200 rounded-md focus:outline-none focus:ring-1 focus:ring-blue-400"
              />
            </div>

            <div className="space-y-3">
              {draft.sections.map((section, i) => (
                <div key={i} className="border border-gray-100 rounded-lg p-3 relative">
                  <button
                    onClick={() => removeSection(i)}
                    disabled={draft.sections.length <= 1}
                    className="absolute top-2 right-2 text-gray-300 hover:text-red-500 disabled:opacity-30"
                  >
                    <X className="w-3.5 h-3.5" />
                  </button>
                  <div className="grid grid-cols-2 gap-2 mb-2">
                    <input
                      value={section.title}
                      onChange={(e) => updateSection(i, { title: e.target.value })}
                      placeholder="Section title (e.g. Action Items)"
                      className="px-2 py-1 text-xs border border-gray-200 rounded"
                    />
                    <select
                      value={section.format}
                      onChange={(e) => updateSection(i, { format: e.target.value as TemplateSection['format'] })}
                      className="px-2 py-1 text-xs border border-gray-200 rounded bg-white"
                    >
                      <option value="paragraph">Paragraph</option>
                      <option value="list">List</option>
                      <option value="string">Short string</option>
                    </select>
                  </div>
                  <textarea
                    value={section.instruction}
                    onChange={(e) => updateSection(i, { instruction: e.target.value })}
                    placeholder="Instruction for the AI: what should go in this section?"
                    rows={2}
                    className="w-full px-2 py-1 text-xs border border-gray-200 rounded resize-none"
                  />
                </div>
              ))}
              <button
                onClick={addSection}
                className="flex items-center gap-1.5 text-xs text-gray-500 hover:text-gray-700 px-2 py-1"
              >
                <Plus className="w-3.5 h-3.5" /> Add section
              </button>
            </div>

            {validationError && (
              <div className="mt-3 text-xs text-red-600 bg-red-50 border border-red-100 rounded-md p-2">
                {validationError}
              </div>
            )}

            <div className="flex justify-end mt-4 pt-3 border-t border-gray-100">
              <button
                onClick={save}
                disabled={isSaving || !draft.name.trim() || draft.sections.length === 0}
                className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50"
              >
                {isSaving && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
                Save template
              </button>
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
