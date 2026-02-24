import { useState, useEffect, useRef, useCallback } from 'react';

function FolderIcon() {
  return (
    <svg className="w-4 h-4 text-yellow-500 shrink-0" viewBox="0 0 20 20" fill="currentColor">
      <path d="M2 6a2 2 0 012-2h5l2 2h5a2 2 0 012 2v6a2 2 0 01-2 2H4a2 2 0 01-2-2V6z" />
    </svg>
  );
}

function FileIcon() {
  return (
    <svg className="w-4 h-4 text-gray-500 shrink-0" fill="none" viewBox="0 0 20 20" stroke="currentColor" strokeWidth={1.5}>
      <path strokeLinecap="round" strokeLinejoin="round" d="M4 4a2 2 0 012-2h4.586a1 1 0 01.707.293l4.414 4.414a1 1 0 01.293.707V16a2 2 0 01-2 2H6a2 2 0 01-2-2V4z" />
      <path strokeLinecap="round" strokeLinejoin="round" d="M10 2v4a2 2 0 002 2h4" />
    </svg>
  );
}

function formatSize(bytes) {
  if (bytes == null) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function Column({ col, index, selections, onDirClick, onFileClick }) {
  if (col.loading) {
    return (
      <div className="w-56 shrink-0 border-r border-gray-800 flex items-center justify-center">
        <div className="w-5 h-5 border-2 border-gray-600 border-t-indigo-400 rounded-full animate-spin" />
      </div>
    );
  }

  if (!col.entries || col.entries.length === 0) {
    return (
      <div className="w-56 shrink-0 border-r border-gray-800 flex items-center justify-center">
        <span className="text-xs text-gray-600">Empty directory</span>
      </div>
    );
  }

  return (
    <div className="w-56 shrink-0 border-r border-gray-800 overflow-y-auto">
      {col.entries.map((entry) => {
        const isSelected = selections[index] === entry.name;
        const isDir = entry.kind === 'directory';
        return (
          <button
            key={entry.name}
            className={`w-full flex items-center gap-2 px-2 py-1 text-left text-sm truncate ${
              isSelected
                ? 'bg-indigo-600/20 text-indigo-300'
                : 'text-gray-300 hover:bg-gray-800'
            }`}
            onClick={() => isDir ? onDirClick(index, entry) : onFileClick(index, entry)}
          >
            {isDir ? <FolderIcon /> : <FileIcon />}
            <span className="truncate flex-1">{entry.name}</span>
            {isDir && <span className="text-gray-600 text-xs shrink-0">&rsaquo;</span>}
          </button>
        );
      })}
    </div>
  );
}

export default function FilesPanel({ sendRaw, subscribe, connected }) {
  const [columns, setColumns] = useState([{ path: '', entries: [], loading: true }]);
  const [selections, setSelections] = useState([]);
  const [selectedFile, setSelectedFile] = useState(null);
  const columnsRef = useRef(null);

  // Subscribe to WS messages and request root listing on mount
  useEffect(() => {
    if (!subscribe || !sendRaw) return;

    const unsubDir = subscribe('directory_listing', (data) => {
      setColumns(prev => prev.map(col =>
        col.path === (data.path ?? '') ? { ...col, entries: data.entries || [], loading: false } : col
      ));
    });

    const unsubFile = subscribe('file_content', (data) => {
      setSelectedFile({
        path: data.path,
        content: data.content ?? '',
        size: data.size,
        truncated: !!data.truncated,
        loading: false,
      });
    });

    // Request root listing
    sendRaw({ type: 'list_directory', path: '' });

    return () => {
      unsubDir();
      unsubFile();
    };
  }, [subscribe, sendRaw]);

  // Auto-scroll columns container to the right when columns change
  useEffect(() => {
    if (columnsRef.current) {
      columnsRef.current.scrollLeft = columnsRef.current.scrollWidth;
    }
  }, [columns.length]);

  const handleDirClick = useCallback((colIndex, entry) => {
    const parentPath = columns[colIndex]?.path ?? '';
    const newPath = parentPath ? `${parentPath}/${entry.name}` : entry.name;

    setColumns(prev => [
      ...prev.slice(0, colIndex + 1),
      { path: newPath, entries: [], loading: true },
    ]);
    setSelections(prev => {
      const next = prev.slice(0, colIndex);
      next[colIndex] = entry.name;
      return next;
    });
    setSelectedFile(null);
    sendRaw({ type: 'list_directory', path: newPath });
  }, [columns, sendRaw]);

  const handleFileClick = useCallback((colIndex, entry) => {
    const parentPath = columns[colIndex]?.path ?? '';
    const filePath = parentPath ? `${parentPath}/${entry.name}` : entry.name;

    setColumns(prev => prev.slice(0, colIndex + 1));
    setSelections(prev => {
      const next = prev.slice(0, colIndex);
      next[colIndex] = entry.name;
      return next;
    });
    setSelectedFile({ path: filePath, content: '', loading: true });
    sendRaw({ type: 'read_file', path: filePath });
  }, [columns, sendRaw]);

  const handleBreadcrumbReset = useCallback(() => {
    setColumns([{ path: '', entries: [], loading: true }]);
    setSelections([]);
    setSelectedFile(null);
    sendRaw({ type: 'list_directory', path: '' });
  }, [sendRaw]);

  // Build breadcrumb text
  const breadcrumbParts = selections.filter(Boolean);

  if (!connected) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center bg-gray-900 text-gray-500">
        <span className="text-sm">Disconnected</span>
      </div>
    );
  }

  const fileName = selectedFile?.path?.split('/').pop() || '';

  return (
    <div className="flex-1 flex flex-col bg-gray-900 min-h-0">
      {/* Breadcrumb bar */}
      <div className="h-10 px-4 flex items-center gap-1 border-b border-gray-800 shrink-0 text-sm text-gray-400 overflow-hidden">
        <button
          className="hover:text-indigo-300 shrink-0"
          onClick={handleBreadcrumbReset}
        >
          workspace
        </button>
        {breadcrumbParts.map((part, i) => (
          <span key={i} className="flex items-center gap-1 shrink-0">
            <span className="text-gray-600">/</span>
            <span className="text-gray-300">{part}</span>
          </span>
        ))}
      </div>

      {/* Main area */}
      <div className="flex-1 flex min-h-0">
        {/* Columns container */}
        <div
          ref={columnsRef}
          className="flex overflow-x-auto min-w-0"
          style={{ flex: selectedFile ? '0 0 60%' : '1 1 100%' }}
        >
          {columns.map((col, i) => (
            <Column
              key={col.path + ':' + i}
              col={col}
              index={i}
              selections={selections}
              onDirClick={handleDirClick}
              onFileClick={handleFileClick}
            />
          ))}
        </div>

        {/* File preview pane */}
        {selectedFile && (
          <div className="flex-1 flex flex-col border-l border-gray-800 min-w-[300px]">
            {/* Preview header */}
            <div className="h-10 px-4 flex items-center justify-between border-b border-gray-800 shrink-0">
              <span className="text-sm text-gray-300 truncate">{fileName}</span>
              <div className="flex items-center gap-2 text-xs text-gray-500 shrink-0">
                {selectedFile.size != null && <span>{formatSize(selectedFile.size)}</span>}
                {selectedFile.truncated && <span className="text-yellow-600">truncated</span>}
              </div>
            </div>
            {/* Preview body */}
            <div className="flex-1 overflow-auto p-4">
              {selectedFile.loading ? (
                <div className="flex items-center justify-center h-full">
                  <div className="w-5 h-5 border-2 border-gray-600 border-t-indigo-400 rounded-full animate-spin" />
                </div>
              ) : (
                <pre className="text-xs text-gray-300 font-mono whitespace-pre-wrap break-words leading-relaxed">
                  {selectedFile.content}
                </pre>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
