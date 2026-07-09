'use client';

import React from 'react';

interface MainContentProps {
  children: React.ReactNode;
}

/**
 * Content pane beside the sidebar. The sidebar is a real flex sibling that
 * owns its own width (resizable, hideable), so no margin bookkeeping here —
 * `relative` anchors overlays (recording pill, status toasts) to the
 * content area instead of the viewport.
 */
const MainContent: React.FC<MainContentProps> = ({ children }) => {
  return (
    <main className="relative flex-1 min-w-0 h-screen overflow-hidden">
      {children}
    </main>
  );
};

export default MainContent;
