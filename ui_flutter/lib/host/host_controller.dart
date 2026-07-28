import 'package:flutter/foundation.dart';

import 'host_client.dart';

class HostController extends ChangeNotifier {
  HostController(this.client) {
    client.onEvent = _onEvent;
  }

  final HostClient client;
  bool connected = false;
  String? lastError;
  Map<String, dynamic>? doctor;
  Map<String, dynamic>? prefs;
  final List<String> logs = [];
  final Map<String, Map<String, dynamic>> jobs = {};

  Future<void> bootstrap() async {
    try {
      if (!client.isRunning) {
        await client.start();
      }
      connected = true;
      doctor = await client.call('app.doctor');
      prefs = await client.call('prefs.get');
      lastError = null;
    } catch (e) {
      connected = false;
      lastError = e.toString();
    }
    notifyListeners();
  }

  Future<Map<String, dynamic>> call(
    String method, [
    Map<String, dynamic>? params,
  ]) async {
    try {
      final result = await client.call(method, params);
      lastError = null;
      if (result is Map<String, dynamic>) return result;
      if (result is Map) return result.cast<String, dynamic>();
      return {'value': result};
    } catch (e) {
      lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  Future<List<Map<String, dynamic>>> callList(
    String method, [
    Map<String, dynamic>? params,
  ]) async {
    final result = await client.call(method, params);
    if (result is List) {
      return result.map((e) => (e as Map).cast<String, dynamic>()).toList();
    }
    if (result is Map && result['items'] is List) {
      return (result['items'] as List)
          .map((e) => (e as Map).cast<String, dynamic>())
          .toList();
    }
    return [];
  }

  Future<void> reloadPrefs() async {
    prefs = await call('prefs.get');
    notifyListeners();
  }

  Future<void> savePrefs(Map<String, dynamic> next) async {
    await call('prefs.set', next);
    prefs = next;
    notifyListeners();
  }

  void pushLog(String line) {
    logs.insert(0, line);
    if (logs.length > 500) logs.removeLast();
    notifyListeners();
  }

  void _onEvent(Map<String, dynamic> event) {
    final kind = event['event']?.toString();
    if (kind == 'job_progress' || kind == 'JobProgress') {
      final jobId = event['job_id']?.toString() ?? '';
      jobs[jobId] = event;
      pushLog(event['message']?.toString() ?? 'progress');
    } else if (kind == 'job_finished' || kind == 'JobFinished') {
      final jobId = event['job_id']?.toString() ?? '';
      jobs[jobId] = event;
      pushLog(event['message']?.toString() ?? 'finished');
    } else if (kind == 'log_append' || kind == 'LogAppend') {
      pushLog(event['line']?.toString() ?? '');
    }
    notifyListeners();
  }
}
