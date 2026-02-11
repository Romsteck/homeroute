String formatSize(int bytes) {
  if (bytes >= 1024 * 1024) {
    final mb = bytes / (1024 * 1024);
    return '${mb.toStringAsFixed(1)} MB';
  }
  final kb = (bytes / 1024).round();
  return '$kb KB';
}
