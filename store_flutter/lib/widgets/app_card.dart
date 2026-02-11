import 'package:flutter/material.dart';
import '../theme.dart';
import '../utils/format_size.dart';

class AppCard extends StatelessWidget {
  final Map<String, dynamic> app;
  final VoidCallback onTap;

  const AppCard({super.key, required this.app, required this.onTap});

  @override
  Widget build(BuildContext context) {
    final name = app['name'] as String? ?? '';
    final category = app['category'] as String? ?? 'other';
    final latestVersion = app['latest_version'] as String?;
    final latestSizeBytes = app['latest_size_bytes'] as int?;
    final releaseCount = app['release_count'] as int? ?? 0;

    return InkWell(
      onTap: onTap,
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
            child: Row(
              children: [
                Container(
                  width: 40,
                  height: 40,
                  color: const Color(0xFF1E3A5F),
                  child: const Icon(
                    Icons.widgets_outlined,
                    color: AppColors.primary,
                    size: 22,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        name,
                        style: const TextStyle(
                          fontSize: 15,
                          fontWeight: FontWeight.w600,
                          color: AppColors.textPrimary,
                        ),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      const SizedBox(height: 2),
                      Text(
                        category,
                        style: const TextStyle(
                          fontSize: 12,
                          color: AppColors.textTertiary,
                        ),
                      ),
                    ],
                  ),
                ),
                Column(
                  crossAxisAlignment: CrossAxisAlignment.end,
                  children: [
                    Text(
                      latestVersion != null ? 'v$latestVersion' : '\u2014',
                      style: const TextStyle(
                        fontSize: 13,
                        color: AppColors.textSecondary,
                        fontFamily: 'monospace',
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      '${latestSizeBytes != null ? formatSize(latestSizeBytes) : ''}'
                      '${latestSizeBytes != null ? ' \u00b7 ' : ''}$releaseCount rel.',
                      style: const TextStyle(
                        fontSize: 11,
                        color: AppColors.textTertiary,
                      ),
                    ),
                  ],
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
