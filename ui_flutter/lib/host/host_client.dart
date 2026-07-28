import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

typedef HostEventHandler = void Function(Map<String, dynamic> event);

/// NDJSON JSON-RPC client over `imgforge-host` stdio.
class HostClient {
  Process? _process;
  IOSink? _stdin;
  final _pending = <dynamic, Completer<dynamic>>{};
  int _nextId = 1;
  HostEventHandler? onEvent;
  bool get isRunning => _process != null;

  Future<void> start({String? hostPath}) async {
    if (_process != null) return;
    final path = hostPath ??
        Platform.environment['IMGFORGE_HOST'] ??
        _defaultHostPath();
    final file = File(path);
    if (!await file.exists()) {
      throw StateError('imgforge-host not found at $path. Build with:\n'
          '  cargo build --features host --bin imgforge-host\n'
          'and set IMGFORGE_HOST.');
    }
    debugPrint('Starting host: $path');
    final process = await Process.start(
      path,
      const [],
      workingDirectory: File(path).parent.parent.parent.path, // repo root-ish
      runInShell: false,
    );
    _process = process;
    _stdin = process.stdin;
    process.stdout
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(_onLine, onError: (e) => debugPrint('host stdout error: $e'));
    process.stderr
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen((line) => debugPrint('[host] $line'));
    process.exitCode.then((code) {
      debugPrint('host exited: $code');
      _process = null;
      _stdin = null;
    });
    await call('app.ping');
  }

  String _defaultHostPath() {
    final root = Directory.current.path;
    final candidates = [
      '$root/../target/debug/imgforge-host',
      '$root/target/debug/imgforge-host',
      '$root/../target/release/imgforge-host',
    ];
    for (final c in candidates) {
      if (File(c).existsSync()) return c;
    }
    return candidates.first;
  }

  Future<void> dispose() async {
    try {
      _stdin?.writeln('exit');
      await _stdin?.flush();
    } catch (_) {}
    _process?.kill();
    _process = null;
  }

  Future<dynamic> call(
    String method, [
    Map<String, dynamic>? params,
  ]) async {
    final stdin = _stdin;
    if (stdin == null || _process == null) {
      throw StateError('imgforge-host is not running');
    }
    final id = _nextId++;
    final completer = Completer<dynamic>();
    _pending[id] = completer;
    final payload = jsonEncode({
      'jsonrpc': '2.0',
      'id': id,
      'method': method,
      'params': params ?? <String, dynamic>{},
    });
    stdin.writeln(payload);
    await stdin.flush();
    return completer.future.timeout(const Duration(minutes: 30));
  }

  void _onLine(String line) {
    if (line.trim().isEmpty) return;
    Map<String, dynamic> msg;
    try {
      msg = jsonDecode(line) as Map<String, dynamic>;
    } catch (e) {
      debugPrint('host bad json: $line');
      return;
    }
    if (msg['method'] == 'host.event') {
      final params = (msg['params'] as Map?)?.cast<String, dynamic>() ?? {};
      onEvent?.call(params);
      return;
    }
    final id = msg['id'];
    final completer = _pending.remove(id);
    if (completer == null) return;
    if (msg['error'] != null) {
      final err = msg['error'] as Map;
      completer.completeError(HostRpcException(
        code: err['code'] as int? ?? -1,
        message: err['message']?.toString() ?? 'unknown error',
      ));
    } else {
      completer.complete(msg['result']);
    }
  }
}

class HostRpcException implements Exception {
  HostRpcException({required this.code, required this.message});
  final int code;
  final String message;
  @override
  String toString() => 'HostRpcException($code): $message';
}
