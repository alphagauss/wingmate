import org.gradle.api.file.ArchiveOperations
import org.gradle.api.file.FileSystemOperations
import org.gradle.api.file.RelativePath
import javax.inject.Inject

plugins {
    id("com.android.application")
}

abstract class UnpackLocalRuntime @Inject constructor(
    private val archives: ArchiveOperations,
    private val files: FileSystemOperations,
) : DefaultTask() {
    @get:InputFile abstract val bundle: RegularFileProperty
    @get:OutputDirectory abstract val assets: DirectoryProperty
    @get:OutputDirectory abstract val libraries: DirectoryProperty

    @TaskAction
    fun unpack() {
        val tree = archives.zipTree(bundle)
        for ((prefix, output) in listOf("assets" to assets, "jniLibs" to libraries)) {
            files.sync {
                from(tree) {
                    include("$prefix/**")
                    eachFile {
                        relativePath = RelativePath(!isDirectory, *relativePath.segments.drop(1).toTypedArray())
                    }
                    includeEmptyDirs = false
                }
                into(output)
            }
        }
    }
}

val unpackLocalRuntime = tasks.register<UnpackLocalRuntime>("unpackLocalRuntime") {
    bundle.set(rootProject.layout.projectDirectory.file("../../../work/build/android/runtime-arm64-v8a.zip"))
    assets.set(layout.buildDirectory.dir("generated/localRuntime/assets"))
    libraries.set(layout.buildDirectory.dir("generated/localRuntime/jniLibs"))
}

android {
    namespace = "dev.wingmate.mobile"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.wingmate.mobile"
        minSdk = 24
        targetSdk = 36
        versionCode = 1001
        versionName = "0.1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    flavorDimensions += "execution"
    productFlavors {
        create("remote") {
            dimension = "execution"
        }
        create("local") {
            dimension = "execution"
            applicationIdSuffix = ".local"
            minSdk = 26
            targetSdk = 28
            ndk { abiFilters += "arm64-v8a" }
        }
    }

    androidResources.noCompress += "xz"
    packaging.jniLibs.apply {
        useLegacyPackaging = true
        keepDebugSymbols += listOf("**/libproot*.so", "**/libtalloc.so", "**/libandroid-shmem.so")
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
        }
    }
}

androidComponents.onVariants(androidComponents.selector().withFlavor("execution" to "local")) { variant ->
    variant.sources.assets?.addGeneratedSourceDirectory(unpackLocalRuntime, UnpackLocalRuntime::assets)
    variant.sources.jniLibs?.addGeneratedSourceDirectory(unpackLocalRuntime, UnpackLocalRuntime::libraries)
}

dependencies {
    "localImplementation"(project(":local-agent"))
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("androidx.drawerlayout:drawerlayout:1.2.0")
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
}
