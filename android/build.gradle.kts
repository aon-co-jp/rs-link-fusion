// RS-Link-Fusion Android shell: single-Activity Kotlin app that launches the
// cross-compiled `rs-linkfusion` native binary (built via `cargo ndk`, TUNを
// 使わない`serve`/`connect`ポート転送モードのみ、`tun-gateway`feature無し
// ——CLAUDE.md 2026-08-05 HANDOFF参照)経由でボンディングを行う。
//
// 参照実装: ../../open-easy-web/android・../../open-web-server/android
// (同じ設計思想、パッケージ名のみ tokyo.runo.rslinkfusion として区別)。
plugins {
    id("com.android.application") version "8.7.2" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
}
