import 'package:flutter_secure_storage/flutter_secure_storage.dart';

const _serverUrlKey = 'server_url';
const _defaultServerUrl = 'https://proxy.mynetwk.biz';
const _storage = FlutterSecureStorage();

Future<String?> getServerUrl() async {
  return await _storage.read(key: _serverUrlKey) ?? _defaultServerUrl;
}

Future<void> setServerUrl(String url) async {
  final clean = url.replaceAll(RegExp(r'/+$'), '');
  await _storage.write(key: _serverUrlKey, value: clean);
}
