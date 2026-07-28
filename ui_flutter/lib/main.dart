import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'app.dart';
import 'host/host_client.dart';
import 'host/host_controller.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final host = HostClient();
  await host.start();
  runApp(
    ChangeNotifierProvider(
      create: (_) => HostController(host)..bootstrap(),
      child: const ImgForgeApp(),
    ),
  );
}
