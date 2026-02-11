import 'package:flutter/material.dart';
import '../theme.dart';

class UpdateBanner extends StatelessWidget {
  final String version;
  final VoidCallback? onTap;
  final VoidCallback? onDismiss;

  const UpdateBanner({
    super.key,
    required this.version,
    this.onTap,
    this.onDismiss,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
            child: Row(
              children: [
                const Icon(Icons.refresh, size: 18, color: AppColors.success),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    'Version $version disponible',
                    style: const TextStyle(
                      color: AppColors.success,
                      fontSize: 14,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
                GestureDetector(
                  onTap: onDismiss,
                  behavior: HitTestBehavior.opaque,
                  child: const Padding(
                    padding: EdgeInsets.all(4),
                    child: Icon(
                      Icons.close,
                      size: 18,
                      color: AppColors.textTertiary,
                    ),
                  ),
                ),
              ],
            ),
          ),
          const Divider(height: 1),
        ],
      ),
    );
  }
}
