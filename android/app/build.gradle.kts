plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "tokyo.runo.rslinkfusion"
    compileSdk = 35

    defaultConfig {
        applicationId = "tokyo.runo.rslinkfusion"
        // WiFi+USB-Ethernet同時保持には`ConnectivityManager.requestNetwork()`
        // の複数`NetworkRequest`同時保持(API 21+)で足りるが、
        // `NetworkCapabilities.TRANSPORT_ETHERNET`をUSB-Ethernetアダプタ
        // 接続時に確実に報告する挙動を期待してAPI 24を最低ラインとした
        // (open-easy-web/open-web-server両android版と同じminSdk方針)。
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        // 実機(arm64-v8a)+この開発機のx86_64エミュレータの両対応
        // (他リポジトリのandroid版と同じ理由)。
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        viewBinding = false
    }

    // ネイティブライブラリをAPK内から直接実行させず、旧来通り
    // nativeLibraryDir配下に展開させる(ProcessBuilderで実ファイルパス
    // として起動する必要があるため、他リポジトリと同じ理由)。
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
}
