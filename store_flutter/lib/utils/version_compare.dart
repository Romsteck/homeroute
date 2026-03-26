bool versionNewer(String a, String b) {
  final partsA = a.split('.').map((s) => int.tryParse(s) ?? 0).toList();
  final partsB = b.split('.').map((s) => int.tryParse(s) ?? 0).toList();
  final len = partsA.length > partsB.length ? partsA.length : partsB.length;
  for (var i = 0; i < len; i++) {
    final va = i < partsA.length ? partsA[i] : 0;
    final vb = i < partsB.length ? partsB[i] : 0;
    if (va > vb) return true;
    if (va < vb) return false;
  }
  return false;
}
