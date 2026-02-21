package com.homeroute.home

import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.core.content.FileProvider
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File

class MainActivity : FlutterActivity() {
    companion object {
        private const val CHANNEL = "com.homeroute.home/package_checker"
        private const val INSTALL_REQUEST_CODE = 1001
    }

    private var pendingInstallResult: MethodChannel.Result? = null
    private var pendingInstallPackage: String? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CHANNEL)
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "isPackageInstalled" -> {
                        val packageName = call.argument<String>("packageName")
                        if (packageName == null) {
                            result.error("INVALID_ARG", "packageName is required", null)
                            return@setMethodCallHandler
                        }
                        val installed = try {
                            packageManager.getPackageInfo(packageName, 0)
                            true
                        } catch (e: PackageManager.NameNotFoundException) {
                            false
                        }
                        result.success(installed)
                    }
                    "installApk" -> {
                        val filePath = call.argument<String>("filePath")
                        if (filePath == null) {
                            result.error("INVALID_ARG", "filePath is required", null)
                            return@setMethodCallHandler
                        }
                        pendingInstallPackage = call.argument<String>("androidPackage")
                        installApk(filePath, result)
                    }
                    "launchApp" -> {
                        val pkg = call.argument<String>("packageName")
                        if (pkg == null) {
                            result.error("INVALID_ARG", "packageName is required", null)
                            return@setMethodCallHandler
                        }
                        val launchIntent = packageManager.getLaunchIntentForPackage(pkg)
                        if (launchIntent != null) {
                            startActivity(launchIntent)
                            result.success(true)
                        } else {
                            result.success(false)
                        }
                    }
                    "openAppSettings" -> {
                        try {
                            val intent = Intent(
                                Settings.ACTION_APPLICATION_DETAILS_SETTINGS
                            ).apply {
                                data = Uri.parse("package:$packageName")
                                flags = Intent.FLAG_ACTIVITY_NEW_TASK
                            }
                            startActivity(intent)
                            result.success(null)
                        } catch (e: Exception) {
                            result.error("SETTINGS_FAILED", e.message, null)
                        }
                    }
                    else -> result.notImplemented()
                }
            }
    }

    private fun installApk(filePath: String, result: MethodChannel.Result) {
        try {
            val file = File(filePath)
            val uri = FileProvider.getUriForFile(
                this,
                "$packageName.fileprovider",
                file
            )
            pendingInstallResult = result
            val intent = Intent(Intent.ACTION_INSTALL_PACKAGE).apply {
                setDataAndType(uri, "application/vnd.android.package-archive")
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                putExtra(Intent.EXTRA_RETURN_RESULT, true)
            }
            startActivityForResult(intent, INSTALL_REQUEST_CODE)
        } catch (e: Exception) {
            pendingInstallResult = null
            result.error("INSTALL_ERROR", e.message, null)
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == INSTALL_REQUEST_CODE) {
            val result = pendingInstallResult
            val pkg = pendingInstallPackage
            pendingInstallResult = null
            pendingInstallPackage = null

            if (resultCode == RESULT_OK) {
                result?.success(true)
            } else {
                // Many devices return RESULT_CANCELED even on successful install.
                // Double-check via PackageManager if we know the package name.
                val actuallyInstalled = if (pkg != null) {
                    try {
                        packageManager.getPackageInfo(pkg, 0)
                        true
                    } catch (e: PackageManager.NameNotFoundException) {
                        false
                    }
                } else {
                    false
                }
                result?.success(actuallyInstalled)
            }
        }
    }
}
