repositories {
    mavenCentral()
}

val ktfmt = configurations.create("ktfmt")

dependencies {
    add(ktfmt.name, "org.jetbrains.kotlinx:ktfmt:0.65:with-dependencies") {
        isTransitive = false
    }
}

val kotlinSources =
    fileTree("../../apps/mobile/android") {
        include("*/src/**/*.kt", "*.gradle.kts", "*/build.gradle.kts")
    } + files("build.gradle.kts", "settings.gradle.kts")

tasks.register<JavaExec>("formatKotlin") {
    classpath = ktfmt
    mainClass.set("org.jetbrains.kotlinx.ktfmt.cli.Main")
    args("--kotlinlang-style", "--do-not-remove-unused-imports")
    args(kotlinSources.files.sorted().map { it.absolutePath })
}

tasks.register<JavaExec>("checkKotlinFormat") {
    classpath = ktfmt
    mainClass.set("org.jetbrains.kotlinx.ktfmt.cli.Main")
    args(
        "--kotlinlang-style",
        "--do-not-remove-unused-imports",
        "--dry-run",
        "--set-exit-if-changed",
    )
    args(kotlinSources.files.sorted().map { it.absolutePath })
}
