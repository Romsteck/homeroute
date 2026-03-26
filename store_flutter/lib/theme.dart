import 'package:flutter/material.dart';

class AppColors {
  static const primary = Color(0xFF60A5FA);
  static const success = Color(0xFF34D399);
  static const error = Color(0xFFEF4444);

  static const background = Color(0xFF111827);
  static const surface = Color(0xFF1F2937);
  static const border = Color(0xFF374151);

  static const textPrimary = Color(0xFFF9FAFB);
  static const textSecondary = Color(0xFF9CA3AF);
  static const textTertiary = Color(0xFF6B7280);

  // Update banner colors (emerald)
  static const updateBannerBg = Color(0xFF064E3B); // emerald-900
  static const updateBannerBorder = Color(0xFF065F46); // emerald-800
  static const updateBannerText = Color(0xFFA7F3D0); // emerald-200

  // Error banner colors (red)
  static const errorBannerBg = Color(0xFF450A0A); // red-950
  static const errorBannerBorder = Color(0xFF7F1D1D); // red-900
}

ThemeData buildAppTheme() {
  return ThemeData(
    brightness: Brightness.dark,
    scaffoldBackgroundColor: AppColors.background,
    primaryColor: AppColors.primary,
    colorScheme: const ColorScheme.dark(
      primary: AppColors.primary,
      secondary: AppColors.primary,
      surface: AppColors.surface,
      error: AppColors.error,
      onPrimary: AppColors.background,
      onSecondary: AppColors.background,
      onSurface: AppColors.textPrimary,
      onError: AppColors.textPrimary,
    ),
    appBarTheme: const AppBarTheme(
      backgroundColor: AppColors.background,
      foregroundColor: AppColors.textPrimary,
      elevation: 0,
    ),
    cardTheme: const CardThemeData(
      color: AppColors.background,
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.zero,
      ),
    ),
    inputDecorationTheme: const InputDecorationTheme(
      filled: true,
      fillColor: AppColors.background,
      border: OutlineInputBorder(
        borderRadius: BorderRadius.zero,
        borderSide: BorderSide(color: AppColors.border),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.zero,
        borderSide: BorderSide(color: AppColors.border),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.zero,
        borderSide: BorderSide(color: AppColors.primary),
      ),
      labelStyle: TextStyle(color: AppColors.textSecondary),
      hintStyle: TextStyle(color: AppColors.textTertiary),
    ),
    textTheme: const TextTheme(
      bodyLarge: TextStyle(color: AppColors.textPrimary),
      bodyMedium: TextStyle(color: AppColors.textPrimary),
      bodySmall: TextStyle(color: AppColors.textSecondary),
      titleLarge: TextStyle(color: AppColors.textPrimary),
      titleMedium: TextStyle(color: AppColors.textPrimary),
      titleSmall: TextStyle(color: AppColors.textSecondary),
      labelLarge: TextStyle(color: AppColors.textPrimary),
    ),
    dividerColor: AppColors.border,
    dividerTheme: const DividerThemeData(
      color: AppColors.border,
      thickness: 1,
      space: 0,
    ),
  );
}
