plugins {
    id("com.android.library")
}

android {
    namespace = "dev.wingmate.mobile.runtime"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    api("androidx.lifecycle:lifecycle-livedata-core:2.10.0")
    implementation("org.tukaani:xz:1.10")
    testImplementation("junit:junit:4.13.2")
}
