export default function DeleteConfirmModal({ onConfirm, onCancel }) {
  return (
    <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50" onClick={onCancel}>
      <div className="bg-gray-800 border border-gray-700 rounded-lg shadow-xl p-5 w-full max-w-sm" onClick={e => e.stopPropagation()}>
        <h3 className="text-lg font-semibold text-gray-200 mb-2">Confirmer la suppression</h3>
        <p className="text-sm text-gray-400 mb-4">Cette action est irreversible. Voulez-vous supprimer cet enregistrement ?</p>
        <div className="flex justify-end gap-2">
          <button onClick={onCancel} className="px-4 py-1.5 text-sm text-gray-400 hover:text-gray-200 bg-gray-700 rounded">
            Annuler
          </button>
          <button onClick={onConfirm} className="px-4 py-1.5 text-sm bg-red-600 hover:bg-red-500 text-white rounded">
            Supprimer
          </button>
        </div>
      </div>
    </div>
  );
}
