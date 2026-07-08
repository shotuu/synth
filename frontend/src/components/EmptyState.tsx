'use client';

import { motion } from 'framer-motion';
import { LucideIcon } from 'lucide-react';
import { ReactNode } from 'react';

interface EmptyStateProps {
  icon: LucideIcon;
  title: string;
  description?: string;
  action?: ReactNode;
}

/**
 * Shared empty-state treatment (icon + heading + description + optional
 * action) so every "nothing here yet" screen reads as one consistent
 * design language rather than plain centered text in some places and a
 * fuller treatment in others (PROJECT_BRIEF.md §8 on empty states mattering).
 */
export function EmptyState({ icon: Icon, title, description, action }: EmptyStateProps) {
  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
      className="flex flex-col items-center justify-center py-16 px-8 text-center"
    >
      <Icon className="w-12 h-12 text-gray-300 mb-4" />
      <h3 className="text-base font-semibold text-gray-800 mb-1.5">{title}</h3>
      {description && <p className="text-sm text-gray-500 max-w-sm mb-4">{description}</p>}
      {action}
    </motion.div>
  );
}
