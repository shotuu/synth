"use client";

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Pencil, Plus, Trash2, Loader2, X, LayoutTemplate } from 'lucide-react';
import { SimpleSelect } from '@/components/ui/simple-select';

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
 * Custom summary template management (PROJECT_BRIEF.md §13: "you get full
 * prompt-editing access to your own templates by default"). Lives in
 * Settings, not a session's header — a template edited here reshapes every
 * future summary that uses it, so it's a global preference, not a
 * per-session control. Built-in templates are read-only sources for
 * duplication; only custom ones can be edited/deleted.
 */
export function TemplateManager() {
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
    loadList();
  }, []);

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
        toast.error("Can't delete a built-in template - only custom ones can be removed here");
        return;
      }
      toast.success('Template deleted');
      if (editingId === id) startNew();
      await loadList();
    } catch (error) {
      console.error('Failed to delete template:', error);
      toast.error('Failed to delete template');
    }
  };

  return (
    <div className="flex gap-6 min-h-[28rem]">
      {/* Template list */}
      <div className="w-56 shrink-0 border-r border-gray-200 pr-4">
        <button
          onClick={startNew}
          className="flex items-center gap-1.5 w-full px-2.5 py-2 mb-2 text-sm font-medium text-blue-600 hover:bg-blue-50 rounded-md"
        >
          <Plus className="w-4 h-4" /> New template
        </button>
        {isLoadingList ? (
          <div className="text-sm text-gray-400 px-2.5">Loading…</div>
        ) : (
          templates.map((t) => (
            <div
              key={t.id}
              className={`group flex items-center gap-1 px-2.5 py-2 rounded-md text-sm cursor-pointer ${
                editingId === t.id ? 'bg-gray-100 font-medium' : 'hover:bg-gray-50'
              }`}
              onClick={() => startEdit(t.id)}
              title={t.description}
            >
              <span className="flex-1 truncate">{t.name}</span>
              <button
                onClick={(e) => { e.stopPropagation(); startDuplicate(t.id); }}
                className="opacity-0 group-hover:opacity-100 text-gray-400 hover:text-blue-600 shrink-0"
                title="Duplicate as a new custom template"
              >
                <Plus className="w-3.5 h-3.5" />
              </button>
            </div>
          ))
        )}
      </div>

      {/* Editor */}
      <div className="flex-1 min-w-0">
        {!editingId && !draft.name && draft.sections.length === 1 && !draft.sections[0].title ? (
          <div className="flex flex-col items-center justify-center h-full text-center text-gray-400 py-16">
            <LayoutTemplate className="w-8 h-8 mb-3 opacity-50" />
            <p className="text-sm">Select a template to edit, or start a new one.</p>
          </div>
        ) : (
          <>
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-sm font-semibold text-gray-800">
                {editingId ? 'Edit Template' : 'New Template'}
              </h3>
              {editingId && (
                <button
                  onClick={() => remove(editingId)}
                  className="flex items-center gap-1 text-sm text-red-500 hover:text-red-700"
                >
                  <Trash2 className="w-4 h-4" /> Delete
                </button>
              )}
            </div>

            <div className="space-y-3 mb-4">
              <input
                value={draft.name}
                onChange={(e) => setDraft((p) => ({ ...p, name: e.target.value }))}
                placeholder="Template name"
                className="w-full px-3 py-2 text-sm border border-gray-200 rounded-md focus:outline-none focus:ring-1 focus:ring-blue-400"
              />
              <input
                value={draft.description}
                onChange={(e) => setDraft((p) => ({ ...p, description: e.target.value }))}
                placeholder="Short description shown in the template picker"
                className="w-full px-3 py-2 text-sm border border-gray-200 rounded-md focus:outline-none focus:ring-1 focus:ring-blue-400"
              />
            </div>

            <div className="space-y-3">
              {draft.sections.map((section, i) => (
                <div key={i} className="border border-gray-200 rounded-lg p-3 relative">
                  <button
                    onClick={() => removeSection(i)}
                    disabled={draft.sections.length <= 1}
                    className="absolute top-2 right-2 text-gray-300 hover:text-red-500 disabled:opacity-30"
                  >
                    <X className="w-4 h-4" />
                  </button>
                  <div className="grid grid-cols-2 gap-2 mb-2">
                    <input
                      value={section.title}
                      onChange={(e) => updateSection(i, { title: e.target.value })}
                      placeholder="Section title (e.g. Action Items)"
                      className="px-2.5 py-1.5 text-sm border border-gray-200 rounded"
                    />
                    <SimpleSelect
                      value={section.format}
                      onValueChange={(v) => updateSection(i, { format: v as TemplateSection['format'] })}
                      options={[
                        { value: 'paragraph', label: 'Paragraph' },
                        { value: 'list', label: 'List' },
                        { value: 'string', label: 'Short string' },
                      ]}
                      className="px-2.5 py-1.5 text-sm border-gray-200 rounded bg-white h-auto"
                    />
                  </div>
                  <textarea
                    value={section.instruction}
                    onChange={(e) => updateSection(i, { instruction: e.target.value })}
                    placeholder="Instruction for the AI: what should go in this section?"
                    rows={2}
                    className="w-full px-2.5 py-1.5 text-sm border border-gray-200 rounded resize-none"
                  />
                </div>
              ))}
              <button
                onClick={addSection}
                className="flex items-center gap-1.5 text-sm text-gray-500 hover:text-gray-700 px-2.5 py-1.5"
              >
                <Plus className="w-4 h-4" /> Add section
              </button>
            </div>

            {validationError && (
              <div className="mt-3 text-sm text-red-600 bg-red-50 border border-red-100 rounded-md p-2.5">
                {validationError}
              </div>
            )}

            <div className="flex justify-end mt-4 pt-3 border-t border-gray-100">
              <button
                onClick={save}
                disabled={isSaving || !draft.name.trim() || draft.sections.length === 0}
                className="flex items-center gap-1.5 px-3.5 py-2 text-sm font-medium rounded-md bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50"
              >
                {isSaving && <Loader2 className="w-4 h-4 animate-spin" />}
                Save template
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
