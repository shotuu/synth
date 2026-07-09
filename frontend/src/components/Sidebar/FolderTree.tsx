'use client';

import React, { useState } from 'react';
import { ChevronRight, Folder as FolderIcon, Inbox, Pencil, Trash2, Check } from 'lucide-react';
import { CONTEXT_STYLES, ContextType } from '@/components/MeetingDetails/ContextTypeSelector';

export interface TreeItem {
  id: string;
  title: string;
  type: 'folder' | 'file';
  children?: TreeItem[];
  contextType?: string;
  icon?: string | null;
  isRealFolder?: boolean;
}

export interface TranscriptMatch {
  id: string;
  matchContext: string;
}

interface FolderTreeProps {
  items: TreeItem[];
  expandedFolders: Set<string>;
  onToggleFolder: (id: string) => void;
  activeMeetingId?: string;
  onOpenItem: (item: TreeItem) => void;
  transcriptMatches?: TranscriptMatch[];
  // Folder management
  onRenameFolder: (folderId: string, name: string) => void;
  onDeleteFolder: (folderId: string) => void;
  // Meeting management
  onRenameMeeting: (meetingId: string, currentTitle: string) => void;
  onDeleteMeeting: (meetingId: string) => void;
  // Drag and drop (meetings into folders, folders into folders)
  onDropIntoFolder: (draggedJson: string, targetFolderId: string | null) => void;
}

/**
 * Obsidian-style collapsible tree for the sidebar (Phase 10): chevron
 * expand/collapse, indent guides instead of boxed rows, hover-revealed
 * actions, drag-and-drop onto folder rows, and an accent left border for
 * the active session. Pure presentation — every mutation goes through a
 * callback so the parent owns data flow.
 */
export function FolderTree(props: FolderTreeProps) {
  return (
    <div className="select-none">
      {props.items.map((item) => (
        <TreeNode key={item.id} item={item} depth={0} {...props} />
      ))}
    </div>
  );
}

function TreeNode({
  item,
  depth,
  ...props
}: FolderTreeProps & { item: TreeItem; depth: number }) {
  const [renaming, setRenaming] = useState(false);
  const [renameValue, setRenameValue] = useState(item.title);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [dragOver, setDragOver] = useState(false);

  const isExpanded = props.expandedFolders.has(item.id);
  const isUnfiled = item.id === 'meetings';
  const match = props.transcriptMatches?.find((m) => m.id === item.id);

  if (item.type === 'folder') {
    const commitRename = () => {
      setRenaming(false);
      const name = renameValue.trim();
      if (name && name !== item.title) props.onRenameFolder(item.id, name);
    };

    return (
      <div>
        <div
          draggable={!!item.isRealFolder && !renaming}
          onDragStart={(e) => {
            e.dataTransfer.effectAllowed = 'move';
            e.dataTransfer.setData('application/json', JSON.stringify({ id: item.id, kind: 'folder' }));
          }}
          onDragOver={(e) => {
            e.preventDefault();
            setDragOver(true);
          }}
          onDragLeave={() => setDragOver(false)}
          onDrop={(e) => {
            e.preventDefault();
            setDragOver(false);
            props.onDropIntoFolder(e.dataTransfer.getData('application/json'), isUnfiled ? null : item.id);
          }}
          onClick={() => props.onToggleFolder(item.id)}
          className={`group flex items-center gap-1 h-7 px-1.5 rounded cursor-pointer text-[13px]
            ${dragOver ? 'bg-blue-50 ring-1 ring-blue-400' : 'hover:bg-gray-100'}`}
          style={{ paddingLeft: `${depth * 14 + 4}px` }}
        >
          <ChevronRight
            className={`w-3.5 h-3.5 shrink-0 text-gray-400 transition-transform duration-100 ${
              isExpanded ? 'rotate-90' : ''
            }`}
          />
          {isUnfiled ? (
            <Inbox className="w-3.5 h-3.5 shrink-0 text-gray-400" />
          ) : item.icon ? (
            <span className="text-xs shrink-0 leading-none">{item.icon}</span>
          ) : (
            <FolderIcon className="w-3.5 h-3.5 shrink-0 text-gray-400" />
          )}
          {renaming ? (
            <input
              autoFocus
              value={renameValue}
              onClick={(e) => e.stopPropagation()}
              onChange={(e) => setRenameValue(e.target.value)}
              onBlur={commitRename}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitRename();
                if (e.key === 'Escape') setRenaming(false);
              }}
              className="flex-1 min-w-0 text-[13px] bg-transparent border-b border-blue-400 outline-none"
            />
          ) : (
            <span className="flex-1 min-w-0 truncate text-gray-600 font-medium">{item.title}</span>
          )}

          {item.isRealFolder && !renaming && (
            <span className="hidden group-hover:flex items-center gap-0.5 shrink-0">
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setRenameValue(item.title);
                  setRenaming(true);
                }}
                className="p-0.5 rounded text-gray-400 hover:text-gray-700"
                aria-label="Rename folder"
              >
                <Pencil className="w-3 h-3" />
              </button>
              {confirmingDelete ? (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setConfirmingDelete(false);
                    props.onDeleteFolder(item.id);
                  }}
                  className="p-0.5 rounded text-red-500"
                  aria-label="Confirm delete folder"
                  title="Click again to confirm"
                >
                  <Check className="w-3 h-3" />
                </button>
              ) : (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setConfirmingDelete(true);
                  }}
                  className="p-0.5 rounded text-gray-400 hover:text-red-500"
                  aria-label="Delete folder"
                >
                  <Trash2 className="w-3 h-3" />
                </button>
              )}
            </span>
          )}
        </div>

        {isExpanded && item.children && (
          // Obsidian-style indent guide: a hairline down the left of nested content
          <div
            className="border-l border-gray-200"
            style={{ marginLeft: `${depth * 14 + 10}px` }}
          >
            <div className="-ml-[1px]">
              {item.children.length === 0 ? (
                <div
                  className="text-[11px] text-gray-400 h-6 flex items-center"
                  style={{ paddingLeft: `${14 + 4}px` }}
                >
                  Empty
                </div>
              ) : (
                item.children.map((child) => (
                  <TreeNode key={child.id} item={child} depth={1} {...props} />
                ))
              )}
            </div>
          </div>
        )}
      </div>
    );
  }

  // Session (file) row
  const isActive = props.activeMeetingId === item.id;
  const contextStyle =
    item.contextType && CONTEXT_STYLES[item.contextType as ContextType]
      ? CONTEXT_STYLES[item.contextType as ContextType]
      : null;

  return (
    <div>
      <div
        draggable
        onDragStart={(e) => {
          e.dataTransfer.effectAllowed = 'move';
          e.dataTransfer.setData('application/json', JSON.stringify({ id: item.id, kind: 'meeting' }));
        }}
        onClick={() => props.onOpenItem(item)}
        className={`group flex items-center gap-1.5 h-7 pr-1.5 rounded-r cursor-pointer text-[13px] border-l-2
          ${
            isActive
              ? 'border-blue-500 bg-blue-50 text-gray-900'
              : 'border-transparent text-gray-600 hover:bg-gray-100 hover:text-gray-800'
          }`}
        style={{ paddingLeft: `${depth * 14 + 8}px` }}
        title={item.title}
      >
        {contextStyle && (
          <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${contextStyle.dot}`} title={contextStyle.label} />
        )}
        <span className="flex-1 min-w-0 truncate">{item.title}</span>
        <span className="hidden group-hover:flex items-center gap-0.5 shrink-0">
          <button
            onClick={(e) => {
              e.stopPropagation();
              props.onRenameMeeting(item.id, item.title);
            }}
            className="p-0.5 rounded text-gray-400 hover:text-gray-700"
            aria-label="Rename session"
          >
            <Pencil className="w-3 h-3" />
          </button>
          <button
            onClick={(e) => {
              e.stopPropagation();
              props.onDeleteMeeting(item.id);
            }}
            className="p-0.5 rounded text-gray-400 hover:text-red-500"
            aria-label="Delete session"
          >
            <Trash2 className="w-3 h-3" />
          </button>
        </span>
      </div>
      {match && (
        <div
          className="mb-0.5 text-[11px] text-gray-500 line-clamp-2 pr-2"
          style={{ paddingLeft: `${depth * 14 + 16}px` }}
        >
          …{match.matchContext}…
        </div>
      )}
    </div>
  );
}
