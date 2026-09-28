import java.awt.Image
import java.awt.image.BufferedImage
import java.io.File
import java.net.URI
import javax.imageio.ImageIO

plugins {
  id("com.android.application")
}

android {
  namespace = "sh.cydonia.remote"
  compileSdk = 36

  defaultConfig {
    applicationId = "sh.cydonia.remote"
    minSdk = 28
    targetSdk = 36
    versionCode = 1
    versionName = "0.1.0"
  }

  buildTypes {
    release {
      isMinifyEnabled = false
      signingConfig = signingConfigs.getByName("debug")
    }
  }

  compileOptions {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
  }
}

kotlin {
  jvmToolchain(17)
}

dependencies {
  testImplementation("junit:junit:4.13.2")
}

abstract class LauncherIcon : DefaultTask() {
  @get:Input abstract val source: Property<String>
  @get:Input abstract val url: Property<String>
  @get:OutputDirectory abstract val outputDir: DirectoryProperty

  @TaskAction
  fun draw() {
    val file = File(source.get())
    if (!file.exists()) {
      file.parentFile.mkdirs()
      URI(url.get()).toURL().openStream().use { input -> file.outputStream().use { input.copyTo(it) } }
    }
    val tile = ImageIO.read(file)
    val densities = listOf("mdpi" to 108, "hdpi" to 162, "xhdpi" to 216, "xxhdpi" to 324, "xxxhdpi" to 432)
    for ((density, side) in densities) {
      val inner = side * 80 / 108
      val offset = (side - inner) / 2
      val canvas = BufferedImage(side, side, BufferedImage.TYPE_INT_ARGB)
      val graphics = canvas.createGraphics()
      graphics.drawImage(tile.getScaledInstance(inner, inner, Image.SCALE_SMOOTH), offset, offset, null)
      graphics.dispose()
      val folder = outputDir.get().asFile.resolve("mipmap-$density")
      folder.mkdirs()
      ImageIO.write(canvas, "png", folder.resolve("ic_launcher_foreground.png"))
    }
  }
}

androidComponents {
  onVariants { variant ->
    val icon = tasks.register<LauncherIcon>("${variant.name}LauncherIcon") {
      source.set(rootProject.file("../assets/icon.png").absolutePath)
      url.set("https://cdn.crabtalk.ai/logos/cydonia.png")
    }
    variant.sources.res?.addGeneratedSourceDirectory(icon, LauncherIcon::outputDir)
  }
}
