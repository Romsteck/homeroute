import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import '../theme.dart';
import '../services/api_client.dart';
import '../services/install_tracker.dart';
import '../services/package_checker.dart';
import '../utils/format_size.dart';
import '../widgets/progress_card.dart';

bool _versionNewer(String a, String b) {
  final pa = a.split('.').map((s) => int.tryParse(s) ?? 0).toList();
  final pb = b.split('.').map((s) => int.tryParse(s) ?? 0).toList();
  final len = pa.length > pb.length ? pa.length : pb.length;
  for (int i = 0; i < len; i++) {
    final va = i < pa.length ? pa[i] : 0;
    final vb = i < pb.length ? pb[i] : 0;
    if (va > vb) return true;
    if (va < vb) return false;
  }
  return false;
}

class _DownloadState {
  final String version;
  final String phase; // 'download', 'install', 'error'
  final double progress;
  final String? error;

  _DownloadState({
    required this.version,
    required this.phase,
    this.progress = 0.0,
    this.error,
  });

  _DownloadState copyWith({
    String? phase,
    double? progress,
    String? error,
  }) {
    return _DownloadState(
      version: version,
      phase: phase ?? this.phase,
      progress: progress ?? this.progress,
      error: error ?? this.error,
    );
  }
}

class AppDetailScreen extends StatefulWidget {
  final String slug;
  final String? name;

  const AppDetailScreen({super.key, required this.slug, this.name});

  @override
  State<AppDetailScreen> createState() => _AppDetailScreenState();
}

class _AppDetailScreenState extends State<AppDetailScreen> {
  Map<String, dynamic>? _app;
  bool _loading = true;
  dynamic _installed; // null = loading, false = not installed, InstalledInfo = installed
  _DownloadState? _dlState;

  @override
  void initState() {
    super.initState();
    _fetchData();
  }

  Future<void> _fetchData() async {
    try {
      final data = await ApiClient.instance.getStoreApp(widget.slug);
      final appData = data['app'] as Map<String, dynamic>?;
      if (!mounted) return;
      setState(() => _app = appData);

      final inst = await InstallTracker.getInstalled(
        widget.slug,
        appData?['android_package'] as String?,
      );
      if (mounted) setState(() => _installed = inst ?? false);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Erreur: $e'), backgroundColor: AppColors.error),
        );
      }
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _showUnknownSourcesDialog() async {
    return showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppColors.surface,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
        title: const Text(
          'Sources inconnues',
          style: TextStyle(color: AppColors.textPrimary, fontSize: 16, fontWeight: FontWeight.w700),
        ),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: const [
            Text(
              'Pour installer cet APK, il faut autoriser les sources inconnues :',
              style: TextStyle(color: AppColors.textSecondary, fontSize: 13, height: 1.5),
            ),
            SizedBox(height: 12),
            Text(
              '1. Paramètres → Applications\n'
              '2. Trouver "HomeRoute Store"\n'
              '3. Activer "Installer des applis inconnues"',
              style: TextStyle(
                color: AppColors.textPrimary,
                fontSize: 13,
                height: 1.7,
                fontFamily: 'monospace',
              ),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Compris', style: TextStyle(color: AppColors.primary)),
          ),
        ],
      ),
    );
  }

  Future<void> _handleDownload(String version) async {
    if (_dlState != null) return;

    // Show Unknown Sources dialog for first-time installs
    final installed = _installed;
    final isInstalled = installed is InstalledInfo;
    if (!isInstalled) {
      await _showUnknownSourcesDialog();
      if (!mounted) return;
    }

    setState(() {
      _dlState = _DownloadState(version: version, phase: 'download', progress: 0);
    });

    try {
      final url = ApiClient.instance.getDownloadUrl(widget.slug, version);
      final tempDir = await getTemporaryDirectory();
      final savePath = '${tempDir.path}/${widget.slug}-$version.apk';

      await ApiClient.instance.downloadFile(
        url: url,
        savePath: savePath,
        onProgress: (received, total) {
          if (mounted && total > 0) {
            setState(() => _dlState = _dlState?.copyWith(
              progress: received / total,
            ));
          }
        },
      );

      if (!mounted) return;
      setState(() => _dlState = _dlState?.copyWith(phase: 'install', progress: 1.0));

      final androidPkg = _app?['android_package'] as String?;

      final installed2 = await PackageChecker.installApk(
        savePath,
        androidPackage: androidPkg,
      );
      if (!mounted) return;

      bool onDevice = installed2;
      if (!onDevice && androidPkg != null && androidPkg.isNotEmpty) {
        for (int attempt = 0; attempt < 3; attempt++) {
          await Future.delayed(const Duration(seconds: 1));
          if (!mounted) return;
          onDevice = await PackageChecker.isPackageInstalled(androidPkg);
          if (onDevice) break;
        }
      }

      if (onDevice) {
        await InstallTracker.markInstalled(widget.slug, version);
        if (mounted) {
          setState(() {
            _installed = InstalledInfo(version: version, installedAt: DateTime.now().toIso8601String());
            _dlState = null;
          });
        }
      } else {
        if (mounted) {
          setState(() {
            _dlState = _dlState?.copyWith(
              phase: 'error',
              error: 'Installation annulée ou échouée',
            );
          });
        }
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _dlState = _dlState?.copyWith(phase: 'error', error: e.toString());
        });
      }
    }
  }

  Future<void> _handleOpenApp() async {
    final androidPkg = _app?['android_package'] as String?;
    if (androidPkg == null || androidPkg.isEmpty) return;
    await PackageChecker.launchApp(androidPkg);
  }

  Future<void> _handleUninstall() async {
    try {
      await PackageChecker.openAppSettings();
    } catch (_) {}
    await InstallTracker.markUninstalled(widget.slug);
    if (mounted) setState(() => _installed = false);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(widget.name ?? widget.slug)),
      body: _loading
          ? const Center(
              child: CircularProgressIndicator(color: AppColors.primary),
            )
          : _app == null
              ? Center(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: const [
                      Icon(Icons.error_outline, size: 48, color: AppColors.textTertiary),
                      SizedBox(height: 12),
                      Text(
                        'Application introuvable',
                        style: TextStyle(color: AppColors.textSecondary, fontSize: 15),
                      ),
                    ],
                  ),
                )
              : _buildBody(),
    );
  }

  Widget _buildBody() {
    final app = _app!;
    final releases = List<Map<String, dynamic>>.from(
      (app['releases'] as List?)?.reversed ?? [],
    );
    final InstalledInfo? installedInfo =
        _installed is InstalledInfo ? _installed as InstalledInfo : null;
    final isInstalled = installedInfo != null;

    String? latestVersion;
    bool hasUpdate = false;
    bool isUpToDate = false;
    int? latestSizeBytes;
    if (releases.isNotEmpty) {
      latestVersion = releases[0]['version'] as String;
      latestSizeBytes = releases[0]['size_bytes'] as int?;
      if (isInstalled) {
        final installedVersion = installedInfo.version;
        hasUpdate = _versionNewer(latestVersion, installedVersion);
        isUpToDate = !hasUpdate;
      }
    }

    return RefreshIndicator(
      onRefresh: _fetchData,
      color: AppColors.primary,
      child: SingleChildScrollView(
        physics: const AlwaysScrollableScrollPhysics(),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Divider(height: 1),

            // App header
            _buildHeader(app, releases, installedInfo),
            const Divider(height: 1),

            // Action buttons
            if (releases.isNotEmpty) ...[
              _buildActionRow(latestVersion!, hasUpdate, isUpToDate, isInstalled),
              const Divider(height: 1),
            ],

            // Download progress
            if (_dlState != null)
              ProgressCard(
                phase: _dlState!.phase,
                progress: _dlState!.progress,
                version: _dlState!.version,
                error: _dlState!.error,
                onDismiss: () => setState(() => _dlState = null),
              ),

            // App info tiles
            _buildInfoRow('Package', app['android_package'] as String? ?? '—'),
            _buildInfoRow('Catégorie', app['category'] as String? ?? 'other'),
            if (latestSizeBytes != null)
              _buildInfoRow('Taille', formatSize(latestSizeBytes)),
            if (releases.isNotEmpty)
              _buildInfoRow('Releases', '${releases.length}'),
            const Divider(height: 1),

            // Description
            if (app['description'] != null &&
                (app['description'] as String).isNotEmpty) ...[
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 14, 16, 14),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    const Text(
                      'Description',
                      style: TextStyle(
                        fontSize: 12,
                        color: AppColors.textTertiary,
                        fontWeight: FontWeight.w600,
                        letterSpacing: 0.5,
                      ),
                    ),
                    const SizedBox(height: 8),
                    Text(
                      app['description'] as String,
                      style: const TextStyle(
                        fontSize: 13,
                        color: AppColors.textSecondary,
                        height: 1.6,
                      ),
                    ),
                  ],
                ),
              ),
              const Divider(height: 1),
            ],

            // Releases / Changelog
            if (releases.isNotEmpty) ...[
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 14, 16, 8),
                child: const Text(
                  'Historique des versions',
                  style: TextStyle(
                    fontSize: 12,
                    color: AppColors.textTertiary,
                    fontWeight: FontWeight.w600,
                    letterSpacing: 0.5,
                  ),
                ),
              ),
              ...releases.take(5).map((release) => _buildReleaseRow(release, installedInfo)),
              const SizedBox(height: 24),
            ],

            // Unknown sources hint (for non-installed apps)
            if (!isInstalled && releases.isNotEmpty) ...[
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 20),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    const Icon(Icons.info_outline,
                        size: 14, color: AppColors.textTertiary),
                    const SizedBox(width: 8),
                    Expanded(
                      child: GestureDetector(
                        onTap: _showUnknownSourcesDialog,
                        child: const Text(
                          'Sources inconnues requises pour installer. Appuyez pour les instructions.',
                          style: TextStyle(
                            fontSize: 12,
                            color: AppColors.textTertiary,
                            height: 1.5,
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _buildInfoRow(String label, String value) {
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          child: Row(
            children: [
              Text(
                label,
                style: const TextStyle(
                  fontSize: 13,
                  color: AppColors.textTertiary,
                ),
              ),
              const Spacer(),
              Text(
                value,
                style: const TextStyle(
                  fontSize: 13,
                  color: AppColors.textSecondary,
                  fontFamily: 'monospace',
                ),
              ),
            ],
          ),
        ),
        const Divider(height: 1),
      ],
    );
  }

  Widget _buildReleaseRow(Map<String, dynamic> release, InstalledInfo? installedInfo) {
    final version = release['version'] as String? ?? '?';
    final changelog = release['changelog'] as String?;
    final sizeBytes = release['size_bytes'] as int?;
    final isCurrentInstall = installedInfo?.version == version;

    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 3),
                decoration: BoxDecoration(
                  color: isCurrentInstall
                      ? AppColors.success.withOpacity(0.12)
                      : AppColors.surface,
                  borderRadius: BorderRadius.circular(4),
                  border: Border.all(
                    color: isCurrentInstall
                        ? AppColors.success.withOpacity(0.3)
                        : AppColors.border,
                    width: 1,
                  ),
                ),
                child: Text(
                  'v$version',
                  style: TextStyle(
                    fontSize: 12,
                    color: isCurrentInstall
                        ? AppColors.success
                        : AppColors.textSecondary,
                    fontFamily: 'monospace',
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    if (changelog != null && changelog.isNotEmpty)
                      Text(
                        changelog,
                        style: const TextStyle(
                          fontSize: 12,
                          color: AppColors.textSecondary,
                          height: 1.5,
                        ),
                      )
                    else
                      const Text(
                        'Pas de notes de version.',
                        style: TextStyle(
                          fontSize: 12,
                          color: AppColors.textTertiary,
                          fontStyle: FontStyle.italic,
                        ),
                      ),
                    if (sizeBytes != null) ...[
                      const SizedBox(height: 2),
                      Text(
                        formatSize(sizeBytes),
                        style: const TextStyle(
                          fontSize: 11,
                          color: AppColors.textTertiary,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
              if (isCurrentInstall)
                const Padding(
                  padding: EdgeInsets.only(left: 8),
                  child: Text(
                    'Installé ✓',
                    style: TextStyle(
                      fontSize: 11,
                      color: AppColors.success,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
            ],
          ),
        ),
        const Divider(height: 1),
      ],
    );
  }

  Widget _buildHeader(
    Map<String, dynamic> app,
    List<Map<String, dynamic>> releases,
    InstalledInfo? installedInfo,
  ) {
    final iconPath = app['icon'] as String?;
    final iconUrl = ApiClient.instance.getIconUrl(iconPath);

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 16),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: 64,
            height: 64,
            decoration: BoxDecoration(
              color: const Color(0xFF1E3A5F),
              borderRadius: BorderRadius.circular(14),
              border: Border.all(color: AppColors.border, width: 1),
            ),
            child: ClipRRect(
              borderRadius: BorderRadius.circular(13),
              child: iconUrl != null
                  ? Image.network(
                      iconUrl,
                      width: 64,
                      height: 64,
                      fit: BoxFit.cover,
                      errorBuilder: (_, __, ___) => const Icon(
                        Icons.widgets_rounded,
                        color: AppColors.primary,
                        size: 32,
                      ),
                    )
                  : const Icon(
                      Icons.widgets_rounded,
                      color: AppColors.primary,
                      size: 32,
                    ),
            ),
          ),
          const SizedBox(width: 14),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  app['name'] as String? ?? '',
                  style: const TextStyle(
                    fontSize: 20,
                    fontWeight: FontWeight.w700,
                    color: AppColors.textPrimary,
                  ),
                ),
                const SizedBox(height: 4),
                Text(
                  app['slug'] as String? ?? '',
                  style: const TextStyle(
                    fontSize: 12,
                    color: AppColors.textTertiary,
                    fontFamily: 'monospace',
                  ),
                ),
                if (installedInfo != null) ...[
                  const SizedBox(height: 6),
                  Container(
                    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
                    decoration: BoxDecoration(
                      color: AppColors.success.withOpacity(0.12),
                      borderRadius: BorderRadius.circular(4),
                      border: Border.all(
                        color: AppColors.success.withOpacity(0.3),
                        width: 1,
                      ),
                    ),
                    child: Text(
                      'Installé ✓  v${installedInfo.version}',
                      style: const TextStyle(
                        fontSize: 11,
                        color: AppColors.success,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                ],
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildActionRow(
    String latestVersion,
    bool hasUpdate,
    bool isUpToDate,
    bool isInstalled,
  ) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
      child: Row(
        children: [
          if (!isUpToDate)
            Expanded(
              child: SizedBox(
                height: 46,
                child: ElevatedButton(
                  onPressed: _dlState != null
                      ? null
                      : () => _handleDownload(latestVersion),
                  style: ElevatedButton.styleFrom(
                    backgroundColor: hasUpdate
                        ? const Color(0xFFF97316)  // orange for update
                        : const Color(0xFF2563EB), // blue for install
                    disabledBackgroundColor: (hasUpdate
                            ? const Color(0xFFF97316)
                            : const Color(0xFF2563EB))
                        .withOpacity(0.4),
                    foregroundColor: Colors.white,
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(6),
                    ),
                  ),
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      if (_dlState?.version == latestVersion &&
                          _dlState?.phase == 'download')
                        ...[
                          const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(
                              strokeWidth: 2,
                              color: Colors.white,
                            ),
                          ),
                          const SizedBox(width: 8),
                          Flexible(
                            child: Text(
                              'Téléchargement... ${(_dlState!.progress * 100).round()}%',
                              style: const TextStyle(
                                fontSize: 13,
                                fontWeight: FontWeight.w600,
                              ),
                              overflow: TextOverflow.ellipsis,
                              maxLines: 1,
                            ),
                          ),
                        ]
                      else if (_dlState?.version == latestVersion &&
                          _dlState?.phase == 'install')
                        ...[
                          const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(
                              strokeWidth: 2,
                              color: Colors.white,
                            ),
                          ),
                          const SizedBox(width: 8),
                          const Flexible(
                            child: Text(
                              'Installation...',
                              style: TextStyle(fontSize: 13, fontWeight: FontWeight.w600),
                              overflow: TextOverflow.ellipsis,
                              maxLines: 1,
                            ),
                          ),
                        ]
                      else ...[
                        Icon(
                          hasUpdate ? Icons.system_update_alt : Icons.download_rounded,
                          size: 18,
                        ),
                        const SizedBox(width: 6),
                        Flexible(
                          child: Text(
                            hasUpdate
                                ? 'Màj v$latestVersion'
                                : 'Installer v$latestVersion',
                            style: const TextStyle(
                              fontSize: 13,
                              fontWeight: FontWeight.w600,
                            ),
                            overflow: TextOverflow.ellipsis,
                            maxLines: 1,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
              ),
            ),
          if (isInstalled) ...[
            if (!isUpToDate) const SizedBox(width: 10),
            Expanded(
              child: SizedBox(
                height: 46,
                child: ElevatedButton(
                  onPressed: _handleOpenApp,
                  style: ElevatedButton.styleFrom(
                    backgroundColor: const Color(0xFF059669),
                    foregroundColor: Colors.white,
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(6),
                    ),
                  ),
                  child: const Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.open_in_new, size: 18),
                      SizedBox(width: 6),
                      Flexible(
                        child: Text(
                          'Ouvrir',
                          style: TextStyle(fontSize: 13, fontWeight: FontWeight.w600),
                          overflow: TextOverflow.ellipsis,
                          maxLines: 1,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
            const SizedBox(width: 8),
            SizedBox(
              height: 46,
              child: OutlinedButton(
                onPressed: _handleUninstall,
                style: OutlinedButton.styleFrom(
                  foregroundColor: const Color(0xFFF87171),
                  side: const BorderSide(color: Color(0xFF7F1D1D)),
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(6),
                  ),
                  padding: const EdgeInsets.symmetric(horizontal: 12),
                ),
                child: const Icon(Icons.delete_outline, size: 18),
              ),
            ),
          ],
        ],
      ),
    );
  }
}
