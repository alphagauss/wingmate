package dev.wingmate.mobile.runtime

import android.content.Context
import org.json.JSONObject

internal data class RuntimeManifest(
    val runtimeVersion: String,
    val archive: String,
    val sha256: String,
    val uncompressedBytes: Long,
    val launchArgv: List<String>,
    val launchEnv: Map<String, String>,
    val upgrade: EngineUpgradeSpec,
) {
    companion object {
        fun load(context: Context): RuntimeManifest {
            val json = context.assets.open("runtime/manifest.json").bufferedReader().use {
                JSONObject(it.readText())
            }
            require(json.getInt("schema") == 1 && json.getString("abi") == "arm64-v8a")
            require(json.getString("rootfs") == "rootfs-arm64-v8a.tar.xz")
            val launch = json.getJSONObject("launch")
            val argv = launch.getJSONArray("argv")
            val env = launch.getJSONObject("env")
            return RuntimeManifest(
                runtimeVersion = json.getString("runtimeVersion"),
                archive = "runtime/rootfs-arm64-v8a.tar.xz",
                sha256 = json.getString("rootfsSha256"),
                uncompressedBytes = json.getLong("uncompressedBytes"),
                launchArgv = List(argv.length()) { argv.getString(it) },
                launchEnv = env.keys().asSequence().associateWith(env::getString),
                upgrade = engineUpgradeSpec(json),
            )
        }
    }
}
