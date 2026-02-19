import { ChevronRight } from 'lucide-react';

export default function Breadcrumb({ appSlug, tableName, recordId, onNavigateHome, onNavigateApp, onNavigateTable }) {
  const segments = [];

  // "Data Browser" is always first
  segments.push({ label: 'Data Browser', onClick: onNavigateHome, isCurrent: false });

  if (appSlug) {
    segments.push({ label: appSlug, onClick: () => onNavigateApp(appSlug), isCurrent: false });
  }

  if (appSlug && tableName) {
    segments.push({ label: tableName, onClick: () => onNavigateTable(appSlug, tableName), isCurrent: false });
  }

  if (appSlug && tableName && recordId != null) {
    segments.push({ label: `#${recordId}`, onClick: null, isCurrent: false });
  }

  // Mark the last segment as current
  if (segments.length > 0) {
    segments[segments.length - 1].isCurrent = true;
    segments[segments.length - 1].onClick = null;
  }

  return (
    <div className="flex items-center gap-1.5 text-sm">
      {segments.map((seg, i) => (
        <span key={i} className="flex items-center gap-1.5">
          {i > 0 && <ChevronRight className="w-3.5 h-3.5 text-gray-600" />}
          {seg.isCurrent ? (
            <span className="text-gray-300 font-medium">{seg.label}</span>
          ) : (
            <span
              className="text-blue-400 hover:text-blue-300 cursor-pointer transition-colors"
              onClick={seg.onClick}
            >
              {seg.label}
            </span>
          )}
        </span>
      ))}
    </div>
  );
}
