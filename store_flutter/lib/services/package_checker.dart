import 'package:flutter/services.dart';

class PackageChecker {
  static const _channel = MethodChannel('com.homeroute.home/package_checker');

  static Future<bool> isPackageInstalled(String packageName) async {
    try {
      final result = await _channel.invokeMethod<bool>(
        'isPackageInstalled',
        {'packageName': packageName},
      );
      return result ?? false;
    } on PlatformException {
      return false;
    }
  }

  static Future<bool> installApk(String filePath, {String? androidPackage}) async {
    try {
      final result = await _channel.invokeMethod<bool>(
        'installApk',
        {
          'filePath': filePath,
          if (androidPackage != null) 'androidPackage': androidPackage,
        },
      );
      return result ?? false;
    } on PlatformException {
      return false;
    }
  }

  static Future<bool> launchApp(String packageName) async {
    try {
      final result = await _channel.invokeMethod<bool>(
        'launchApp',
        {'packageName': packageName},
      );
      return result ?? false;
    } on PlatformException {
      return false;
    }
  }

  static Future<void> openAppSettings() async {
    try {
      await _channel.invokeMethod('openAppSettings');
    } on PlatformException {
      // Ignore
    }
  }
}
