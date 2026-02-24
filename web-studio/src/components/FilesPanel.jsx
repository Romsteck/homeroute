export default function FilesPanel() {
  return (
    <div className="flex-1 flex flex-col items-center justify-center bg-gray-900 text-gray-500">
      <svg className="w-16 h-16 mb-4 text-gray-700" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1}>
        <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
      </svg>
      <h3 className="text-lg font-medium text-gray-400 mb-2">File Browser</h3>
      <p className="text-sm text-gray-600">File browsing will be available in a future update.</p>
    </div>
  );
}
