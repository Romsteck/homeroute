export default function LookupLink({ targetTable, value, onClick }) {
  if (value === null || value === undefined) {
    return <span className="text-gray-600 italic">null</span>;
  }

  return (
    <span
      className="text-blue-400 hover:text-blue-300 hover:underline cursor-pointer text-sm"
      title={`Ouvrir ${targetTable} #${value}`}
      onClick={(e) => {
        e.stopPropagation();
        onClick(targetTable, value);
      }}
    >
      {targetTable} #{value}
    </span>
  );
}
