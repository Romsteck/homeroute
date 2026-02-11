import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import '../theme.dart';
import '../services/api_client.dart';
import '../services/package_checker.dart';
import '../utils/format_size.dart';

class UpdateScreen extends StatefulWidget {
  final String currentVersion;
  final String newVersion;
  final String changelog;
  final int sizeBytes;

  const UpdateScreen({
    super.key,
    required this.currentVersion,
    required this.newVersion,
    required this.changelog,
    required this.sizeBytes,
  });

  @override
  State<UpdateScreen> createState() => _UpdateScreenState();
}

class _UpdateScreenState extends State<UpdateScreen> {
  String _phase = 'idle'; // idle, download, install, error
  double _progress = 0.0;
  String? _error;

  Future<void> _handleUpdate() async {
    if (_phase == 'download' || _phase == 'install') return;
    setState(() {
      _phase = 'download';
      _progress = 0.0;
      _error = null;
    });

    try {
      final url = ApiClient.instance.getClientApkUrl();
      final tempDir = await getTemporaryDirectory();
      final savePath = '${tempDir.path}/homeroute-store-${widget.newVersion}.apk';

      await ApiClient.instance.downloadFile(
        url: url,
        savePath: savePath,
        onProgress: (received, total) {
          if (mounted && total > 0) {
            setState(() => _progress = received / total);
          }
        },
      );

      if (!mounted) return;
      setState(() {
        _phase = 'install';
        _progress = 1.0;
      });

      await PackageChecker.installApk(savePath);

      if (mounted) {
        setState(() {
          _phase = 'idle';
          _progress = 0.0;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _phase = 'error';
          _progress = 0.0;
          _error = e.toString();
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final isBusy = _phase == 'download' || _phase == 'install';

    return Scaffold(
      appBar: AppBar(title: const Text('Mise \u00e0 jour')),
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Divider(height: 1),

          // Update icon + title
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 20),
            child: Row(
              children: [
                const Icon(Icons.refresh, size: 32, color: AppColors.success),
                const SizedBox(width: 14),
                const Expanded(
                  child: Text(
                    'Mise \u00e0 jour disponible',
                    style: TextStyle(
                      fontSize: 18,
                      fontWeight: FontWeight.w700,
                      color: AppColors.textPrimary,
                    ),
                  ),
                ),
                if (widget.sizeBytes > 0)
                  Text(
                    formatSize(widget.sizeBytes),
                    style: const TextStyle(
                      fontSize: 13,
                      color: AppColors.textTertiary,
                    ),
                  ),
              ],
            ),
          ),
          const Divider(height: 1),

          // Version comparison
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 16),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      const Text(
                        'ACTUELLE',
                        style: TextStyle(
                          fontSize: 11,
                          color: AppColors.textTertiary,
                          letterSpacing: 0.5,
                        ),
                      ),
                      const SizedBox(height: 4),
                      Text(
                        widget.currentVersion,
                        style: const TextStyle(
                          fontSize: 20,
                          fontWeight: FontWeight.w700,
                          color: AppColors.textPrimary,
                          fontFamily: 'monospace',
                        ),
                      ),
                    ],
                  ),
                ),
                const Padding(
                  padding: EdgeInsets.symmetric(horizontal: 12),
                  child: Icon(
                    Icons.arrow_forward,
                    size: 20,
                    color: AppColors.textTertiary,
                  ),
                ),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      const Text(
                        'NOUVELLE',
                        style: TextStyle(
                          fontSize: 11,
                          color: AppColors.textTertiary,
                          letterSpacing: 0.5,
                        ),
                      ),
                      const SizedBox(height: 4),
                      Text(
                        widget.newVersion,
                        style: const TextStyle(
                          fontSize: 20,
                          fontWeight: FontWeight.w700,
                          color: AppColors.success,
                          fontFamily: 'monospace',
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
          const Divider(height: 1),

          // Changelog
          if (widget.changelog.isNotEmpty) ...[
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Text(
                    'NOUVEAUT\u00c9S',
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.w600,
                      color: AppColors.textTertiary,
                      letterSpacing: 0.5,
                    ),
                  ),
                  const SizedBox(height: 8),
                  Text(
                    widget.changelog,
                    style: const TextStyle(
                      fontSize: 14,
                      color: AppColors.textPrimary,
                      height: 1.5,
                    ),
                  ),
                ],
              ),
            ),
            const Divider(height: 1),
          ],

          // Download button
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
            child: SizedBox(
              width: double.infinity,
              height: 48,
              child: ElevatedButton(
                onPressed: isBusy ? null : _handleUpdate,
                style: ElevatedButton.styleFrom(
                  backgroundColor: const Color(0xFF059669),
                  disabledBackgroundColor: const Color(0xFF059669).withOpacity(0.5),
                  foregroundColor: Colors.white,
                  shape: const RoundedRectangleBorder(
                    borderRadius: BorderRadius.zero,
                  ),
                ),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    if (isBusy)
                      const SizedBox(
                        width: 20,
                        height: 20,
                        child: CircularProgressIndicator(
                          strokeWidth: 2,
                          color: Colors.white,
                        ),
                      )
                    else
                      const Icon(Icons.download, size: 20),
                    const SizedBox(width: 8),
                    Text(
                      _phase == 'download'
                          ? 'T\u00e9l\u00e9chargement ${(_progress * 100).round()}%'
                          : _phase == 'install'
                              ? 'Installation en cours...'
                              : 'T\u00e9l\u00e9charger et installer',
                      style: const TextStyle(
                        fontSize: 15,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),

          // Progress bar during download
          if (_phase == 'download') ...[
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: LinearProgressIndicator(
                value: _progress,
                backgroundColor: AppColors.border,
                valueColor: const AlwaysStoppedAnimation<Color>(Color(0xFF059669)),
                minHeight: 4,
              ),
            ),
            const SizedBox(height: 12),
            const Divider(height: 1),
          ],

          // Error
          if (_phase == 'error') ...[
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: const [
                      Icon(Icons.error, size: 16, color: AppColors.error),
                      SizedBox(width: 8),
                      Text(
                        '\u00c9chec',
                        style: TextStyle(
                          fontSize: 13,
                          color: AppColors.error,
                          fontWeight: FontWeight.w500,
                        ),
                      ),
                    ],
                  ),
                  if (_error != null) ...[
                    const SizedBox(height: 8),
                    Text(
                      _error!,
                      style: const TextStyle(
                        fontSize: 12,
                        color: AppColors.textSecondary,
                        fontFamily: 'monospace',
                        height: 1.4,
                      ),
                    ),
                  ],
                  const SizedBox(height: 10),
                  Align(
                    alignment: Alignment.centerRight,
                    child: GestureDetector(
                      onTap: _handleUpdate,
                      child: const Text(
                        'R\u00e9essayer',
                        style: TextStyle(
                          fontSize: 13,
                          color: AppColors.primary,
                          fontWeight: FontWeight.w500,
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ),
            const Divider(height: 1),
          ],
        ],
      ),
    );
  }
}
