import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package_checker.dart';

const _storageKey = 'installed_apps';

class InstalledInfo {
  final String version;
  final String? installedAt;

  InstalledInfo({required this.version, this.installedAt});

  factory InstalledInfo.fromJson(Map<String, dynamic> json) {
    return InstalledInfo(
      version: json['version'] as String,
      installedAt: json['installedAt'] as String?,
    );
  }

  Map<String, dynamic> toJson() => {
        'version': version,
        if (installedAt != null) 'installedAt': installedAt,
      };
}

class InstallTracker {
  static Future<Map<String, dynamic>> _getAll() async {
    final prefs = await SharedPreferences.getInstance();
    final raw = prefs.getString(_storageKey);
    if (raw == null) return {};
    return jsonDecode(raw) as Map<String, dynamic>;
  }

  static Future<void> _saveAll(Map<String, dynamic> all) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_storageKey, jsonEncode(all));
  }

  static Future<InstalledInfo?> getInstalled(
      String slug, String? androidPackage) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.reload(); // Force reload from disk
    final raw = prefs.getString(_storageKey);
    debugPrint('[InstallTracker] getInstalled($slug, $androidPackage) raw=$raw');
    final Map<String, dynamic> all =
        raw != null ? jsonDecode(raw) as Map<String, dynamic> : {};

    // Native detection via PackageManager
    if (androidPackage != null && androidPackage.isNotEmpty) {
      try {
        final onDevice =
            await PackageChecker.isPackageInstalled(androidPackage);
        debugPrint('[InstallTracker] native check: onDevice=$onDevice');
        if (onDevice) {
          if (all.containsKey(slug)) {
            return InstalledInfo.fromJson(
                all[slug] as Map<String, dynamic>);
          }
          return InstalledInfo(version: 'installed');
        }
      } catch (e) {
        debugPrint('[InstallTracker] native check error: $e');
      }
    }

    // Fallback: SharedPreferences tracking
    debugPrint('[InstallTracker] fallback: containsKey=${ all.containsKey(slug)}');
    if (all.containsKey(slug)) {
      return InstalledInfo.fromJson(all[slug] as Map<String, dynamic>);
    }
    return null;
  }

  static Future<void> markInstalled(String slug, String version) async {
    final all = await _getAll();
    all[slug] = {
      'version': version,
      'installedAt': DateTime.now().toIso8601String(),
    };
    await _saveAll(all);
  }

  static Future<void> markUninstalled(String slug) async {
    final all = await _getAll();
    all.remove(slug);
    await _saveAll(all);
  }
}
