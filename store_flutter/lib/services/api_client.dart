import 'package:dio/dio.dart';
import 'secure_storage.dart' as storage;

class ApiClient {
  ApiClient._();
  static final instance = ApiClient._();

  Dio? _dio;
  String? _baseUrl;

  Future<void> init() async {
    _baseUrl = await storage.getServerUrl();
    if (_baseUrl != null) {
      _dio = Dio(BaseOptions(
        baseUrl: '$_baseUrl/api',
        headers: {'Content-Type': 'application/json'},
      ));
    }
  }

  Future<void> setBaseUrl(String url) async {
    final clean = url.replaceAll(RegExp(r'/+$'), '');
    await storage.setServerUrl(clean);
    _baseUrl = clean;
    _dio = Dio(BaseOptions(
      baseUrl: '$clean/api',
      headers: {'Content-Type': 'application/json'},
    ));
  }

  String? get baseUrl => _baseUrl;

  void _ensureConfigured() {
    if (_dio == null || _baseUrl == null) {
      throw Exception('Serveur non configuré');
    }
  }

  Future<dynamic> getStoreApps() async {
    _ensureConfigured();
    final response = await _dio!.get('/store/apps');
    return response.data;
  }

  Future<dynamic> getStoreApp(String slug) async {
    _ensureConfigured();
    final response = await _dio!.get('/store/apps/$slug');
    return response.data;
  }

  Future<dynamic> checkUpdates(Map<String, String> installed) async {
    _ensureConfigured();
    final param = installed.entries
        .map((e) => '${e.key}:${e.value}')
        .join(',');
    final response = await _dio!.get('/store/updates?installed=$param');
    return response.data;
  }

  Future<dynamic> getClientVersion() async {
    _ensureConfigured();
    final response = await _dio!.get('/store/client/version');
    return response.data;
  }

  String getDownloadUrl(String slug, String version) {
    _ensureConfigured();
    return '$_baseUrl/api/store/releases/$slug/$version/download';
  }

  String getClientApkUrl() {
    _ensureConfigured();
    return '$_baseUrl/api/store/client/apk';
  }

  Future<Response> downloadFile({
    required String url,
    required String savePath,
    void Function(int, int)? onProgress,
    CancelToken? cancelToken,
  }) async {
    _ensureConfigured();
    return await _dio!.download(
      url,
      savePath,
      onReceiveProgress: onProgress,
      cancelToken: cancelToken,
    );
  }
}
