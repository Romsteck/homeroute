import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../theme.dart';
import '../services/api_client.dart';
import '../services/install_tracker.dart';
import '../widgets/app_card.dart';
import '../widgets/error_banner.dart';
import '../widgets/update_banner.dart';

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

class CatalogScreen extends StatefulWidget {
  const CatalogScreen({super.key});

  @override
  State<CatalogScreen> createState() => _CatalogScreenState();
}

class _CatalogScreenState extends State<CatalogScreen> {
  static const _dismissedStoreVersionKey = 'dismissed_store_version';
  List<Map<String, dynamic>> _apps = [];
  bool _loading = true;
  String? _error;
  Map<String, dynamic>? _updateInfo;
  bool _updateDismissed = false;

  Set<String> _appsWithUpdates = {};
  Set<String> _installedSlugs = {};

  // Search + filter
  final _searchController = TextEditingController();
  String _searchQuery = '';
  String _selectedCategory = 'Tous';
  bool _showSearch = false;

  @override
  void initState() {
    super.initState();
    _init();
    _searchController.addListener(() {
      setState(() => _searchQuery = _searchController.text.toLowerCase());
    });
  }

  @override
  void dispose() {
    _searchController.dispose();
    super.dispose();
  }

  Future<void> _init() async {
    _verifySelfUpdate();
    await _fetchApps();
    _checkClientUpdate();
    _checkAppUpdates();
    _loadInstalledSlugs();
  }

  /// After a self-update, the app is killed and restarted. On next launch,
  /// verify that the version actually changed to the expected one.
  Future<void> _verifySelfUpdate() async {
    const key = 'pending_self_update_version';
    try {
      final prefs = await SharedPreferences.getInstance();
      final pendingVersion = prefs.getString(key);
      if (pendingVersion == null) return;

      // Always clear the flag so we don't check repeatedly
      await prefs.remove(key);

      final packageInfo = await PackageInfo.fromPlatform();
      final currentVersion = packageInfo.version;

      if (!mounted) return;

      if (currentVersion == pendingVersion) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Store mis à jour avec succès ✓'),
            backgroundColor: Color(0xFF059669),
            duration: Duration(seconds: 3),
          ),
        );
      } else {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(
              'Échec de la mise à jour du Store '
              '(attendu $pendingVersion, actuel $currentVersion)',
            ),
            backgroundColor: AppColors.error,
            duration: const Duration(seconds: 5),
          ),
        );
      }
    } catch (_) {}
  }

  Future<void> _fetchApps() async {
    setState(() => _error = null);
    try {
      final data = await ApiClient.instance.getStoreApps();
      final apps = (data['apps'] as List?)
              ?.map((e) => Map<String, dynamic>.from(e as Map))
              .toList() ??
          [];
      if (mounted) {
        setState(() {
          _apps = apps;
          _loading = false;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = e.toString();
          _loading = false;
        });
      }
    }
  }

  Future<void> _loadInstalledSlugs() async {
    try {
      final installed = await InstallTracker.getAllInstalled();
      if (mounted) {
        setState(() => _installedSlugs = installed.keys.toSet());
      }
    } catch (_) {}
  }

  Future<void> _checkClientUpdate() async {
    try {
      final data = await ApiClient.instance.getClientVersion();
      final packageInfo = await PackageInfo.fromPlatform();
      final prefs = await SharedPreferences.getInstance();
      final current = packageInfo.version;
      final remote = data['version'] as String?;
      if (remote != null && _versionNewer(remote, current) && mounted) {
        final dismissedVersion =
            prefs.getString(_dismissedStoreVersionKey);
        setState(() {
          _updateInfo = {
            'version': remote,
            'changelog': data['changelog'] ?? '',
            'sizeBytes': data['size_bytes'] ?? 0,
          };
          _updateDismissed = dismissedVersion == remote;
        });
      }
    } catch (_) {}
  }

  Future<void> _dismissUpdateBanner() async {
    final version = _updateInfo?['version'] as String?;
    if (version == null) return;

    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_dismissedStoreVersionKey, version);

    if (!mounted) return;
    setState(() => _updateDismissed = true);
  }

  Future<void> _checkAppUpdates() async {
    try {
      final installed = await InstallTracker.getAllInstalled();
      if (installed.isEmpty) return;
      final valid = Map.fromEntries(
        installed.entries.where((e) => RegExp(r'^\d').hasMatch(e.value)),
      );
      if (valid.isEmpty) return;
      final data = await ApiClient.instance.checkUpdates(valid);
      final updateList = data['updates'] as List? ?? [];
      final updates = <String>{};
      for (final u in updateList) {
        final map = u as Map<String, dynamic>;
        final slug = map['slug'] as String?;
        if (slug != null) updates.add(slug);
        // Also mark the originally-installed slug (may differ when
        // the backend resolved via android_package to a different entry)
        final installedSlug = map['installed_slug'] as String?;
        if (installedSlug != null) updates.add(installedSlug);
      }
      if (mounted) {
        setState(() => _appsWithUpdates = updates);
      }
    } catch (_) {}
  }

  List<String> get _categories {
    final cats = <String>{'Tous'};
    for (final app in _apps) {
      final cat = app['category'] as String?;
      if (cat != null && cat.isNotEmpty) cats.add(cat);
    }
    return cats.toList();
  }

  List<Map<String, dynamic>> get _filteredApps {
    return _apps.where((app) {
      // Category filter
      if (_selectedCategory != 'Tous') {
        final cat = app['category'] as String? ?? '';
        if (cat != _selectedCategory) return false;
      }
      // Search filter
      if (_searchQuery.isNotEmpty) {
        final name = (app['name'] as String? ?? '').toLowerCase();
        final slug = (app['slug'] as String? ?? '').toLowerCase();
        final cat = (app['category'] as String? ?? '').toLowerCase();
        if (!name.contains(_searchQuery) &&
            !slug.contains(_searchQuery) &&
            !cat.contains(_searchQuery)) {
          return false;
        }
      }
      return true;
    }).toList();
  }

  @override
  Widget build(BuildContext context) {
    final cats = _categories;
    final filtered = _filteredApps;

    return Scaffold(
      appBar: AppBar(
        title: _showSearch
            ? TextField(
                controller: _searchController,
                autofocus: true,
                style: const TextStyle(color: AppColors.textPrimary),
                decoration: const InputDecoration(
                  hintText: 'Rechercher...',
                  border: InputBorder.none,
                  enabledBorder: InputBorder.none,
                  focusedBorder: InputBorder.none,
                  hintStyle: TextStyle(color: AppColors.textTertiary),
                  isDense: true,
                  contentPadding: EdgeInsets.zero,
                ),
              )
            : const Text('Store'),
        actions: [
          IconButton(
            icon: Icon(_showSearch ? Icons.close : Icons.search),
            onPressed: () {
              setState(() {
                _showSearch = !_showSearch;
                if (!_showSearch) {
                  _searchController.clear();
                  _searchQuery = '';
                }
              });
            },
          ),
          IconButton(
            icon: const Icon(Icons.settings),
            onPressed: () => context.push('/settings'),
          ),
        ],
      ),
      body: _loading
          ? const Center(
              child: CircularProgressIndicator(color: AppColors.primary),
            )
          : Column(
              children: [
                const Divider(height: 1),
                if (_error != null) ErrorBanner(message: _error!),
                if (_updateInfo != null && !_updateDismissed)
                  UpdateBanner(
                    version: _updateInfo!['version'] as String,
                    onTap: () {
                      PackageInfo.fromPlatform().then((info) {
                        if (!context.mounted) return;
                        context.push('/update', extra: {
                          'currentVersion': info.version,
                          'newVersion': _updateInfo!['version'],
                          'changelog': _updateInfo!['changelog'],
                          'sizeBytes': _updateInfo!['sizeBytes'],
                        });
                      });
                    },
                    onDismiss: _dismissUpdateBanner,
                  ),
                // Category filter tabs (only show if more than 1 category)
                if (cats.length > 2) ...[
                  SizedBox(
                    height: 40,
                    child: ListView.builder(
                      scrollDirection: Axis.horizontal,
                      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                      itemCount: cats.length,
                      itemBuilder: (context, i) {
                        final cat = cats[i];
                        final selected = cat == _selectedCategory;
                        return Padding(
                          padding: const EdgeInsets.only(right: 6),
                          child: GestureDetector(
                            onTap: () => setState(() => _selectedCategory = cat),
                            child: Container(
                              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                              decoration: BoxDecoration(
                                color: selected
                                    ? AppColors.primary.withValues(alpha: 0.15)
                                    : AppColors.surface,
                                borderRadius: BorderRadius.circular(14),
                                border: Border.all(
                                  color: selected ? AppColors.primary : AppColors.border,
                                  width: 1,
                                ),
                              ),
                              child: Text(
                                cat,
                                style: TextStyle(
                                  fontSize: 12,
                                  color: selected
                                      ? AppColors.primary
                                      : AppColors.textSecondary,
                                  fontWeight: selected
                                      ? FontWeight.w600
                                      : FontWeight.normal,
                                ),
                              ),
                            ),
                          ),
                        );
                      },
                    ),
                  ),
                  const Divider(height: 1),
                ],
                // Stats row
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                  child: Row(
                    children: [
                      const Icon(Icons.widgets_outlined,
                          size: 13, color: AppColors.textTertiary),
                      const SizedBox(width: 5),
                      Text(
                        '${filtered.length} app${filtered.length != 1 ? 's' : ''}',
                        style: const TextStyle(
                          fontSize: 12,
                          color: AppColors.textTertiary,
                        ),
                      ),
                      if (_installedSlugs.isNotEmpty) ...[
                        const SizedBox(width: 14),
                        const Icon(Icons.check_circle_outline,
                            size: 13, color: AppColors.textTertiary),
                        const SizedBox(width: 5),
                        Text(
                          '${_installedSlugs.length} installée${_installedSlugs.length != 1 ? 's' : ''}',
                          style: const TextStyle(
                            fontSize: 12,
                            color: AppColors.textTertiary,
                          ),
                        ),
                      ],
                      if (_appsWithUpdates.isNotEmpty) ...[
                        const SizedBox(width: 14),
                        const Icon(Icons.update,
                            size: 13, color: AppColors.success),
                        const SizedBox(width: 5),
                        Text(
                          '${_appsWithUpdates.length} màj',
                          style: const TextStyle(
                            fontSize: 12,
                            color: AppColors.success,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
                const Divider(height: 1),
                Expanded(
                  child: RefreshIndicator(
                    onRefresh: () async {
                      await _fetchApps();
                      await _checkAppUpdates();
                      await _loadInstalledSlugs();
                    },
                    color: AppColors.primary,
                    child: filtered.isEmpty
                        ? ListView(
                            children: [
                              Padding(
                                padding: const EdgeInsets.only(top: 80),
                                child: Column(
                                  children: [
                                    Icon(
                                      _searchQuery.isNotEmpty
                                          ? Icons.search_off
                                          : Icons.storefront_outlined,
                                      size: 48,
                                      color: AppColors.textTertiary,
                                    ),
                                    const SizedBox(height: 12),
                                    Text(
                                      _searchQuery.isNotEmpty
                                          ? 'Aucun résultat pour "$_searchQuery"'
                                          : 'Aucune application',
                                      style: const TextStyle(
                                        color: AppColors.textSecondary,
                                        fontSize: 16,
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                            ],
                          )
                        : ListView.builder(
                            padding: EdgeInsets.zero,
                            itemCount: filtered.length,
                            itemBuilder: (context, index) {
                              final app = filtered[index];
                              final slug = app['slug'] as String;
                              return AppCard(
                                app: app,
                                hasUpdate: _appsWithUpdates.contains(slug),
                                isInstalled: _installedSlugs.contains(slug),
                                onTap: () {
                                  final name = app['name'] as String?;
                                  context.push(
                                    '/app/$slug${name != null ? '?name=${Uri.encodeComponent(name)}' : ''}',
                                  );
                                },
                              );
                            },
                          ),
                  ),
                ),
              ],
            ),
    );
  }
}
