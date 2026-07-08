import { useState, useEffect, useCallback } from 'react';
import { invoke as invokeTauri } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import Analytics from '@/lib/analytics';

export function useTemplates() {
  const [availableTemplates, setAvailableTemplates] = useState<Array<{
    id: string;
    name: string;
    description: string;
  }>>([]);
  const [selectedTemplate, setSelectedTemplate] = useState<string>('standard_meeting');

  const fetchTemplates = useCallback(async () => {
    try {
      const templates = await invokeTauri('api_list_templates') as Array<{
        id: string;
        name: string;
        description: string;
      }>;
      console.log('Available templates:', templates);
      setAvailableTemplates(templates);
    } catch (error) {
      console.error('Failed to fetch templates:', error);
    }
  }, []);

  // Fetch available templates on mount
  useEffect(() => {
    fetchTemplates();
  }, [fetchTemplates]);

  // True once the user has explicitly picked a template; context-type
  // defaulting must never override an explicit choice
  const [userPickedTemplate, setUserPickedTemplate] = useState(false);

  // Handle template selection
  const handleTemplateSelection = useCallback((templateId: string, templateName: string) => {
    setSelectedTemplate(templateId);
    setUserPickedTemplate(true);
    toast.success('Template selected', {
      description: `Using "${templateName}" template for summary generation`,
    });
    Analytics.trackFeatureUsed('template_selected');
  }, []);

  // Silently default the template to match the session's context type
  // (meeting → standard_meeting, lecture → lecture, ...)
  const applyContextTypeDefault = useCallback(
    (contextType: string) => {
      if (userPickedTemplate) return;
      const mapped = CONTEXT_TEMPLATE_MAP[contextType];
      if (mapped) setSelectedTemplate(mapped);
    },
    [userPickedTemplate]
  );

  return {
    availableTemplates,
    selectedTemplate,
    handleTemplateSelection,
    applyContextTypeDefault,
    refetchTemplates: fetchTemplates,
  };
}

const CONTEXT_TEMPLATE_MAP: Record<string, string> = {
  meeting: 'standard_meeting',
  lecture: 'lecture',
  discussion: 'discussion',
  coffee_chat: 'coffee_chat',
  custom: 'standard_meeting',
};
