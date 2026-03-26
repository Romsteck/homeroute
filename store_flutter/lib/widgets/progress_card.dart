import 'package:flutter/material.dart';
import '../theme.dart';

class ProgressCard extends StatelessWidget {
  final String phase; // 'download', 'install', 'error'
  final double progress;
  final String? version;
  final String? error;
  final Color? progressColor;
  final VoidCallback? onDismiss;
  final VoidCallback? onRetry;

  const ProgressCard({
    super.key,
    required this.phase,
    this.progress = 0.0,
    this.version,
    this.error,
    this.progressColor,
    this.onDismiss,
    this.onRetry,
  });

  @override
  Widget build(BuildContext context) {
    final color = progressColor ?? AppColors.primary;

    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
          child: _buildContent(color),
        ),
        const Divider(height: 1),
      ],
    );
  }

  Widget _buildContent(Color color) {
    switch (phase) {
      case 'download':
        return Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Row(
              children: [
                Icon(Icons.cloud_download_outlined, size: 16, color: color),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    'T\u00e9l\u00e9chargement${version != null ? ' v$version' : ''}...',
                    style: const TextStyle(
                      fontSize: 13,
                      color: AppColors.textPrimary,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                ),
                Text(
                  '${(progress * 100).round()}%',
                  style: TextStyle(
                    fontSize: 13,
                    color: color,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 10),
            LinearProgressIndicator(
              value: progress,
              backgroundColor: AppColors.border,
              valueColor: AlwaysStoppedAnimation<Color>(color),
              minHeight: 4,
            ),
          ],
        );
      case 'install':
        return Row(
          children: [
            SizedBox(
              width: 16,
              height: 16,
              child: CircularProgressIndicator(
                strokeWidth: 2,
                valueColor: AlwaysStoppedAnimation<Color>(color),
              ),
            ),
            const SizedBox(width: 8),
            const Text(
              'Installation en cours...',
              style: TextStyle(
                fontSize: 13,
                color: AppColors.textPrimary,
                fontWeight: FontWeight.w500,
              ),
            ),
          ],
        );
      case 'error':
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Icon(Icons.error, size: 16, color: AppColors.error),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    '\u00c9chec${version != null ? ' v$version' : ''}',
                    style: const TextStyle(
                      fontSize: 13,
                      color: AppColors.error,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                ),
              ],
            ),
            if (error != null) ...[
              const SizedBox(height: 8),
              Text(
                error!,
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
                onTap: onDismiss,
                child: const Text(
                  'Fermer',
                  style: TextStyle(
                    fontSize: 13,
                    color: AppColors.textSecondary,
                    fontWeight: FontWeight.w500,
                  ),
                ),
              ),
            ),
          ],
        );
      default:
        return const SizedBox.shrink();
    }
  }
}
