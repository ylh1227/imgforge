import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'app.dart';
import 'host/host_client.dart';
import 'host/host_controller.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final host = HostClient();
  try {
    await host.start();
  } catch (e) {
    debugPrint('Host start deferred: $e');
  }
  runApp(
    ChangeNotifierProvider(
      create: (_) => HostController(host)..bootstrap(),
      child: const ImgForgeApp(),
    ),
  );
}
