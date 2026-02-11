import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:go_router/go_router.dart';
import 'theme.dart';
import 'services/api_client.dart';
import 'screens/catalog_screen.dart';
import 'screens/app_detail_screen.dart';
import 'screens/settings_screen.dart';
import 'screens/update_screen.dart';

final _router = GoRouter(
  initialLocation: '/catalog',
  routes: [
    GoRoute(
      path: '/catalog',
      builder: (context, state) => const CatalogScreen(),
    ),
    GoRoute(
      path: '/app/:slug',
      builder: (context, state) {
        final slug = state.pathParameters['slug']!;
        final name = state.uri.queryParameters['name'];
        return AppDetailScreen(slug: slug, name: name);
      },
    ),
    GoRoute(
      path: '/settings',
      builder: (context, state) => const SettingsScreen(),
    ),
    GoRoute(
      path: '/update',
      builder: (context, state) {
        final extra = state.extra as Map<String, dynamic>;
        return UpdateScreen(
          currentVersion: extra['currentVersion'] as String,
          newVersion: extra['newVersion'] as String,
          changelog: extra['changelog'] as String,
          sizeBytes: extra['sizeBytes'] as int,
        );
      },
    ),
  ],
);

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await SystemChrome.setPreferredOrientations([
    DeviceOrientation.portraitUp,
    DeviceOrientation.portraitDown,
  ]);
  await ApiClient.instance.init();
  runApp(const HomeRouteStoreApp());
}

class HomeRouteStoreApp extends StatelessWidget {
  const HomeRouteStoreApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp.router(
      title: 'HomeRoute Store',
      theme: buildAppTheme(),
      routerConfig: _router,
      debugShowCheckedModeBanner: false,
    );
  }
}
