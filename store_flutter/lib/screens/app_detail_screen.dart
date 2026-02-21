import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import '../theme.dart';
import '../services/api_client.dart';
import '../services/install_tracker.dart';
import '../services/package_checker.dart';
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
  // null = loading, false = not installed, InstalledInfo = installed
  dynamic _installed;
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

  Future<void> _handleDownload(String version) async {
    if (_dlState != null) return;
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

      final installed = await PackageChecker.installApk(
        savePath,
        androidPackage: androidPkg,
      );
      if (!mounted) return;

      // Verify the app is actually on the device.
      // Retry up to 3 times with 1s delays to handle PackageManager
      // registration delays on some devices.
      bool onDevice = installed;
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
            _dlState = _dlState?.copyWith(phase: 'error', error: 'Installation annul\u00e9e ou \u00e9chou\u00e9e');
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
    final InstalledInfo? installedInfo = _installed is InstalledInfo ? _installed as InstalledInfo : null;
    final isInstalled = installedInfo != null;

    String? latestVersion;
    bool hasUpdate = false;
    bool isUpToDate = false;
    if (releases.isNotEmpty) {
      latestVersion = releases[0]['version'] as String;
      if (isInstalled) {
        final installedVersion = installedInfo.version;
        hasUpdate = _versionNewer(latestVersion, installedVersion);
        isUpToDate = !hasUpdate;
      }
    }

    return SingleChildScrollView(
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

          // Progress
          if (_dlState != null)
            ProgressCard(
              phase: _dlState!.phase,
              progress: _dlState!.progress,
              version: _dlState!.version,
              error: _dlState!.error,
              onDismiss: () => setState(() => _dlState = null),
            ),

          // Description
          if (app['description'] != null && (app['description'] as String).isNotEmpty) ...[
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
              child: Text(
                app['description'] as String,
                style: const TextStyle(
                  fontSize: 13,
                  color: AppColors.textSecondary,
                  height: 1.5,
                ),
              ),
            ),
            const Divider(height: 1),
          ],

          const SizedBox(height: 40),
        ],
      ),
    );
  }

  Widget _buildHeader(
    Map<String, dynamic> app,
    List<Map<String, dynamic>> releases,
    InstalledInfo? installedInfo,
  ) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      child: Row(
        children: [
          Container(
            width: 48,
            height: 48,
            color: const Color(0xFF1E3A5F),
            child: const Icon(
              Icons.widgets_outlined,
              color: AppColors.primary,
              size: 26,
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
                    fontSize: 18,
                    fontWeight: FontWeight.w700,
                    color: AppColors.textPrimary,
                  ),
                ),
                const SizedBox(height: 2),
                Row(
                  children: [
                    Text(
                      app['slug'] as String? ?? '',
                      style: const TextStyle(
                        fontSize: 12,
                        color: AppColors.textTertiary,
                        fontFamily: 'monospace',
                      ),
                    ),
                    const SizedBox(width: 12),
                    Text(
                      app['category'] as String? ?? 'other',
                      style: const TextStyle(
                        fontSize: 12,
                        color: AppColors.textTertiary,
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
          if (installedInfo != null)
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
              color: const Color(0xFF064E3B),
              child: Text(
                'v${installedInfo.version}',
                style: const TextStyle(
                  fontSize: 12,
                  color: AppColors.success,
                  fontWeight: FontWeight.w600,
                ),
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
                height: 44,
                child: ElevatedButton(
                  onPressed: _dlState != null
                      ? null
                      : () => _handleDownload(latestVersion),
                  style: ElevatedButton.styleFrom(
                    backgroundColor:
                        hasUpdate ? const Color(0xFF059669) : const Color(0xFF2563EB),
                    disabledBackgroundColor: (hasUpdate
                            ? const Color(0xFF059669)
                            : const Color(0xFF2563EB))
                        .withOpacity(0.5),
                    foregroundColor: Colors.white,
                    shape: const RoundedRectangleBorder(
                      borderRadius: BorderRadius.zero,
                    ),
                  ),
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      if (_dlState?.version == latestVersion &&
                          _dlState?.phase != 'error')
                        const SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(
                            strokeWidth: 2,
                            color: Colors.white,
                          ),
                        )
                      else
                        Icon(
                          hasUpdate ? Icons.refresh : Icons.download,
                          size: 18,
                        ),
                      const SizedBox(width: 8),
                      Text(
                        hasUpdate
                            ? 'Mettre \u00e0 jour v$latestVersion'
                            : 'Installer v$latestVersion',
                        style: const TextStyle(
                          fontSize: 14,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          if (isInstalled) ...[
            if (!isUpToDate) const SizedBox(width: 10),
            Expanded(
              child: SizedBox(
                height: 44,
                child: ElevatedButton(
                  onPressed: _handleOpenApp,
                  style: ElevatedButton.styleFrom(
                    backgroundColor: const Color(0xFF059669),
                    foregroundColor: Colors.white,
                    shape: const RoundedRectangleBorder(
                      borderRadius: BorderRadius.zero,
                    ),
                  ),
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: const [
                      Icon(Icons.open_in_new, size: 18),
                      SizedBox(width: 8),
                      Text(
                        'Ouvrir',
                        style: TextStyle(
                          fontSize: 14,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
            const SizedBox(width: 10),
            SizedBox(
              height: 44,
              child: OutlinedButton(
                onPressed: _handleUninstall,
                style: OutlinedButton.styleFrom(
                  foregroundColor: const Color(0xFFF87171),
                  side: const BorderSide(color: Color(0xFF7F1D1D)),
                  shape: const RoundedRectangleBorder(
                    borderRadius: BorderRadius.zero,
                  ),
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
