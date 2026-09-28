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
