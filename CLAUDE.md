# 開発方針・設計書(RS-Link-Fusion、旧名RS-LinkFusion) — 2026-07-23時点、コアロジック実装・実機検証済み

GitHubリポジトリ: [aon-co-jp/rs-link-fusion](https://github.com/aon-co-jp/rs-link-fusion)
(2026-07-31に`RS-LinkFusion`から改名)。
公開先: `https://easy-web.tokyo/rs-link-fusion`(デモ環境:
`https://easy-web.tokyo/rs-link-fusion/demo`)。
VPS上の作業パス: `/root/rs-link-fusion`。

> ⚠️ **正直な開示**: 前回セッションはリミット接近で`cargo build`/
> `cargo test`未検証のまま中断していたが、本セッションで検証・
> `main.rs`実装・実データ転送の実機検証まで完了した(詳細は
> HANDOFF参照)。GUI/サービス化・macOS/Android/iOS対応は引き続き
> 未着手(ユーザー確認済みのスコープ、下記「対応プラットフォーム」
> 節参照)。

作業ドライブは`F:\runo`。この節は[`open-raid-z`](https://github.com/aon-co-jp/open-raid-z)の
`CLAUDE.md`を正本とし、各プロジェクトへコピーして同期する方針に準じる。

## このプロジェクトの役割

複数のWAN/LAN/WiFi(古い規格〜WiFi 7まで、OSがネットワークインター
フェースとして認識するものはすべて対象)を1つの論理接続へ束ね
(ボンディング)、通信の高速化・安定化を実現する、インストーラー付き
Windows/Linuxアプリ。ユーザー指示「上記通信技術で、CPU＋GPU+NPUが
あればハードウェアアクセラレーター可能として。複数のWANの複数の
LAN＋複数のWiFiは、古いWiFiからWifi7まで対応の融合(ミックス)を可能
として、通信の高速化と安定化などを可能として、インストーラー付き
Windows版とLINUX版アプリとしても、提供してダウンロード可能にして」
に基づく。

## 設計の核心

### 1. 複数インターフェースのボンディング(実装済み・実績のあるライブラリを採用)

`aggligator`(`aggligator-transport-tcp`)クレートを採用。このリポジトリ
自身が新たに実装するのではなく、**既に`open-web-server-wire::mptcp_channel`
で実績のある枯れたクレート**を使う。日英Web検索で裏取り済みの重要な
発見:

- `TcpConnector::set_multi_interface(true)`(**デフォルトで有効、
  Android以外**)が、ローカルマシンが持つ**全ネットワークインター
  フェース**(LAN・WiFi・複数WAN等、OSが認識するものすべて)を自動
  列挙し、各インターフェース×各サーバーIPの組み合わせごとに個別の
  TCPリンクを確立、それらを1つの論理接続へ束ねる。**この機能は
  「LAN」「WiFi」「WAN」を区別しない**——単に「ローカルにあるすべての
  ネットワークインターフェース」を対象にするため、古いWiFi規格〜
  WiFi 7まで、OSがIPを持つインターフェースとして認識してさえいれば
  そのまま対象になる(WiFi規格自体の違いはOS/NICドライバ層で吸収
  される、アプリケーション層で規格ごとの特別対応は不要)。
- 実世界の直接の先行例: **Speedify**(複数のWAN/WiFi/セルラー回線を
  束ねる商用ソフト、Windows/macOS/Linux/iOS/Android対応、専用
  ハードウェア不要)。**OpenMPTCProuter**はカーネルMPTCP非対応環境で
  Glorytun/MLVPNを使う設計——このリポジトリがWindows(カーネルMPTCP
  非対応)で`aggligator`を使うのと同じ判断構造。
- 「品質連動型ルーティング」(ping応答があってもHTTP応答が遅い場合は
  即座にバックアップ回線へ切替)が2026年の到達点とされている
  ([jisaku.com: 回線冗長化・帯域結合](https://jisaku.com/posts/network-bonding-failover-2026))
  ——これを担うのが下記2.の`RS-SmartTCP`統合。

### 2. ネットワーク品質適応(`RS-SmartTCP`、実装済み・別リポジトリ)

[RS-SmartTCP](https://github.com/aon-co-jp/RS-SmartTCP)の
`NetworkQualityMonitor`/`AdaptivePolicy`をそのまま利用し、ボンディング
接続の実効RTT/ジッターを追跡、IOWN/APNのような光ネットワーク級の
リンクを検知した場合と通常インターネット級とで挙動を切り替える
(`quality.rs`)。**正直な開示**: 個々の物理リンク単位の内訳ではなく、
ボンディングされた論理接続全体の実効品質という単純化を採用した
(`aggligator::alc::Stream`が公開するリンク単位統計を使うより粗い
実装、次回以降の高度化候補)。

### 3. CPU/GPU/NPU/専用ハードウェアアクセラレータの抽象化(`accel.rs`)

`open-web-server-wire::accel`と同じ設計判断——`AccelBackend`列挙型
(`Cpu`/`Gpu`/`Npu`/`HardwareAccelerator`)で将来のハードウェアをAPI
形状として先取りし、**`Cpu`のみ実装**(`flate2`圧縮+ChaCha20-Poly1305
暗号化)、他は要求時に安全に`Cpu`へフォールバックする。本リポジトリは
独立した配布物(ダウンロードしてすぐ動く単体バイナリ)であるため、
`open-web-server`への依存を持たせず、同じパターンを自己完結で再実装
した(コード重複はあるが、依存グラフを軽量に保つトレードオフ)。

日英Web検索での裏取り: GPU圧縮(NVIDIA nvCOMP、Snappy/ZSTD/LZ4対応、
Blackwell世代は専用デコンプレッションエンジンで600GB/s)・GPU暗号化
(CUDA上でのAES高速化、学術研究レベルで実例あり)は実在するが、Rust
エコシステムには両者を統合した実用クレートが見当たらず、今回は
CPUバックエンドのみを実装。

### 4. トンネル方式(`framed.rs`、実装済み・未検証)

`[len:u32 LE][圧縮+暗号化済みペイロード]`という単純な長さプレフィクス
フレームで、ボンディング接続上に任意のTCPトラフィックを流す
「ミニVPN的トンネル」。`serve`(リモート側、ローカルサービスへの
リバースプロキシ)と`connect`(ローカル側、ローカルポートで待ち受け
てボンディング接続へ転送)の2つのCLIサブコマンドを想定
(**未実装**、次回セッションで着手)。

## 対応プラットフォーム(現状・計画)

| プラットフォーム | 状況 |
|---|---|
| Windows | 今回の主要ターゲット。`install.ps1`想定(未作成) |
| Linux | 今回の主要ターゲット。`install.sh`想定(未作成) |
| macOS | **計画のみ**——Mac実機購入後に着手する前提(ユーザー確認済み)。インストーラー部分だけ別設計にする方針(ユーザー指示) |
| Android | **`serve`/`connect`(TUN無し)のみビルド成功・APK生成済み(2026-08-05)**——`gateway-*`(TUNフルVPN)はAndroidのVpnServiceモデル非対応のため引き続き対象外。実機のUSB-Ethernetアダプタが無くWiFi+USB-Ethernet同時ボンディングの実機検証は未実施、`CAP_NET_RAW`権限の要否も未検証(詳細はHANDOFF参照) |
| iPhone/iPad(iOS/iPadOS) | **計画のみ**——ビルドにXcode(Mac必須)が必要、macOS対応と同じ制約 |
| スマートTV/4K TV | **計画のみ、スコープ判断を保留**。ユーザー指摘の通りLAN+WiFiを両方持つTVでは技術的にボンディングは意味を持ちうるが、「回線冗長化は本来ルーター/PC側で行うもの」という観点との整合、および対応OS(Android TV/Tizen/webOS)ごとのツールチェーン差異は次回セッションで要検討 |

## 未実装・次回セッションで最優先すべきこと

1. ~~`cargo build`/`cargo test`の実行~~ **完了(2026-07-23)**。
2. ~~`main.rs`の実装~~ **完了(2026-07-23)**。
3. **実際に複数インターフェースを持つマシンでの実機検証**は未着手
   (この開発環境がループバックのみか複数NICを持つかの確認を含む)。
   ループバック上でのserve/connect実データ転送検証は完了済み
   (下記HANDOFF参照)——複数物理NICでの真のマルチホーミング効果
   自体はこのサンドボックスでは検証できていない、という正直な限界。
4. ~~`install.sh`/`install.ps1`の作成~~ **完了(2026-07-23)**。
5. ~~GitHub Releases自動ビルドワークフロー~~ **完了(2026-07-23、
   `.github/workflows/release.yml`)**。タグpushでの実リリース動作
   自体は未検証。
6. ~~README.md/PORTING.mdの作成~~ **完了(2026-07-23)**。
7. ~~GUI/サービス化~~ **一部完了(2026-07-23、後続HANDOFF参照)**:
   `gateway-serve`/`gateway-connect`(TUNゲートウェイ)・QoS・速度測定
   ・GUIを追加実装。ただしいずれも実機検証は未完了(下記参照)。
8. **次に優先すべきこと**: (a) タグpush(`v0.1.0`等)による
   `release.yml`の実動作確認、(b) 複数物理NIC・管理者権限を持つ実機
   でのTUNゲートウェイ・QoS・GUIの実機検証(本セッションはサンドボックス
   の制約で未完了)、(c) ~~GPU/NPUアクセラレーション~~
   **`open-cuda`側でGPU実装候補が完成(2026-07-23、下記追記参照)**、
   `accel.rs::AccelBackend::Gpu`への統合は次回セッションで着手。

- **2026-07-23(続き) `open-cuda`側でGPU圧縮/暗号化カーネル(ChaCha20)の
  実装が完了、`accel.rs::AccelBackend::Gpu`統合の実装候補ができた**:
  `open-cuda`の`opencuda-directx`クレートにChaCha20 GPUカーネル
  (DXIL/HLSL)が実装され、RustCrypto製`chacha20`クレートとの数値一致を
  実機(NVIDIA GT 730)で検証済み(コミット`ec6acf1`、詳細は`open-cuda`
  側CLAUDE.md HANDOFF参照)。**正直な開示・残作業**: (a) これは
  ChaCha20暗号化部分のみで、`accel.rs`が使う完全なAEAD
  (ChaCha20-Poly1305)には認証タグ(Poly1305)のGPU実装が別途必要、
  (b) 小サイズペイロード(トンネルのMTU程度、数百〜数千バイト)での
  H2D/D2Hオーバーヘッドが、GPU演算の優位性を相殺してしまわないかの
  実ベンチマークが未実施、(c) 本リポジトリ側の`AccelBackend::Gpu`は
  依然として`Cpu`へのフォールバックのみ(実際の配線はまだ行っていない)。
  次回セッションでの着手事項として記録。

## HANDOFF

- **2026-08-05 Android版ビルドブロッカーを解消しAndroidアプリシェルを新規実装
  (ユーザー指示「WiFi回線とUSB有線LANアダプタの2回線を同時にボンディング
  したい」、前回HANDOFF「次にすべきこと(1)」への対応)**:
  1. **真の根本原因を実機ビルドで特定(推測ではない)**: 前回HANDOFFの
     仮説一覧(a〜c)のうち、実際に効いたのは(b)に近い形——
     `aggligator-transport-tcp`本体が依存する`network-interface` crate
     (v2.0.5)のソース(`~/.cargo/registry/src/.../network-interface-2.0.5/
     src/target/linux.rs`)を直接確認したところ、`NetworkInterface::show()`
     が`#[cfg(any(target_os = "android", target_os = "linux"))]`という
     Android向け分岐の中で`getifaddrs()`/`freeifaddrs()`(glibc/BSD API)を
     呼んでおり、これがBionic libcにリンクできず`undefined symbol:
     getifaddrs`でリンク失敗することを`cargo ndk -t aarch64-linux-android
     build --release --no-default-features`の実行結果で確認した。
  2. **解決策(c)寄りの対応を採用——`network-interface`をローカルフォーク
     しAndroid向け代替実装に差し替え**: crates.io版をコピーし
     `vendor/network-interface/`として同梱、`src/target/linux.rs`に
     `#[cfg(target_os = "android")] mod android { ... }`を新設。
     `getifaddrs`を一切呼ばず、(a) `/proc/net/dev`からインターフェース名を
     列挙、(b) 自前定義した`ifreq`構造体(`libc`crateはlinux_like全般で
     `ifreq`を公開していないため、Linuxカーネルabiに合わせて手動定義)+
     生の`ioctl(SIOCGIFADDR/SIOCGIFFLAGS)`呼び出しでIPv4アドレス・
     ループバックフラグを取得する方式に置き換えた。**正直な制約**:
     IPv6アドレス・MACアドレス・ブロードキャストアドレスは取得しない
     (IPv4のみ)——WiFi/USB-Ethernetのボンディング用途ではIPv4のみで
     実用上十分と判断し、過剰実装を避けた。`Cargo.toml`に
     `[patch.crates-io] network-interface = { path = "vendor/network-
     interface" }`を追加(全ターゲット・全依存元に適用されるが、
     非Android側の`show()`実装は無変更のため挙動は変わらない)。
  3. **重要な追加の正直な開示(実機検証の限界)**: `aggligator-transport-
     tcp`の出力方向の接続確立(`TcpConnector::connect`)は
     `util::bind_socket_to_interface`→`socket.bind_device(Some(interface))`
     (`SO_BINDTODEVICE`)を**Android/Linux/Fuchsiaでは常に**使う設計
     (`aggligator-transport-tcp-0.2.5/src/util.rs`86行目、ソースで確認済み)。
     `SO_BINDTODEVICE`の設定にはLinuxカーネル上`CAP_NET_RAW`権限が
     必要——一般の(root化していない)Androidアプリはこの権限を持たない
     ため、**インターフェース列挙自体(今回解決した部分)は成功しても、
     実際に2つ目以降のインターフェースへ`bind_device`で結び付けて
     ボンディングリンクを張る部分は、非rootのAndroid実機では`EPERM`で
     失敗する可能性が高い**(この開発環境には実機が無いため実際の
     エラーコードは未確認、Linuxカーネルのケーパビリティ仕様からの
     推測に基づく正直な懸念として明記する)。これが事実であれば、
     Android版の複数インターフェース同時ボンディングは追加の対応
     (root化端末専用にする、またはAndroidの`Network.bindSocket()`相当を
     `rs-linkfusion`本体に統合できるよう別途プロトコルを設計する等)が
     次回以降必要になる、という重要な未解決課題として記録する。
  4. **ビルド実証(実機ビルド、型チェックのみではない)**: `cargo ndk -t
     aarch64-linux-android build --release --no-default-features`・
     `-t armv7-linux-androideabi`・`-t x86_64-linux-android`の3ターゲット
     すべてでリンク成功まで確認した(以前は全ターゲットでリンクエラー
     だった状態から前進)。デスクトップ側の`cargo build --release`・
     `cargo test --release`(19件全green、回帰無し)も引き続き成功する
     ことを確認し、今回のパッチが非Androidターゲットの挙動を変えて
     いないことを実証した。
  5. **Androidアプリシェルを新規実装(`android/`)**: `open-easy-web/
     android`・`open-web-server/android`と同じ設計パターン(単一Activity、
     `cargo ndk`でクロスビルドしたネイティブバイナリを`jniLibs`配下に
     `libRSLinkFusion`相当の名前(`librslinkfusion.so`)で同梱、
     `ProcessBuilder`で起動)。**スコープはユーザー指示通り`serve`/
     `connect`(TUN無し、ポート転送モード)のみ**——同梱バイナリは
     `--no-default-features`(`tun-gateway`feature無し)でビルドしたもの。
     - `MainActivity.kt`: モード選択(`connect`/`serve`)・鍵入力
       (`generate-key`ボタンで生成)・アドレス入力・起動/停止ボタン。
       `rs-linkfusion`自体はHTTPサーバーではなく`/healthz`相当が無い
       ため、起動確認は`connect`モードならローカルリスンアドレスへの
       実TCP接続試行、`serve`モードならプロセス生存確認
       (`Process.isAlive`)という代替手段で行う設計にした(正直な設計
       上の妥協点として明記)。
     - `NetworkBinder.kt`: `ConnectivityManager.requestNetwork()`で
       `TRANSPORT_WIFI`・`TRANSPORT_ETHERNET`の両方を同時要求・保持し、
       `getLinkProperties(network).interfaceName`で実際のインター
       フェース名(`wlan0`/`eth0`等)を画面に表示する。**正直な開示・
       意図的なスコープ限定**: `Network.bindSocket()`はこのJVMプロセス
       内のソケットにしか効かず、別プロセス(`ProcessBuilder`で起動した
       ネイティブバイナリ)のソケットには適用できないため、今回の実装は
       「両ネットワークが実際に存在し使えること」の確認・表示に留め、
       プロセス間でのソケットFD受け渡しやVpnServiceラップのような
       過剰実装は行っていない(`rs-linkfusion`本体は上記2.の
       `NetworkInterface::show()`経由で両インターフェースを自動列挙・
       使用する既存の`multi_interface`設計にそのまま委ねる)。
  6. **Gradleビルド実証**: `gradle :app:assembleDebug --offline`
     **BUILD SUCCESSFUL**(arm64-v8a+x86_64両ABIの`librslinkfusion.so`
     〈約12.9MB/13.4MB、ストリップ不可のため未ストリップのまま同梱、
     ビルドログに"Unable to strip"警告あり・実害無し〉を含む
     `app-debug.apk`が実際に生成されることを確認、`unzip -l`で
     `lib/arm64-v8a/librslinkfusion.so`・`lib/x86_64/librslinkfusion.so`
     の実在を確認済み)。
  7. **正直な開示・未検証事項(誇張しない)**: (a) この開発環境には
     実機のUSB-Ethernetアダプタが無いため、WiFi+USB-Ethernet同時
     ボンディングの実機E2E検証は一切行っていない。(b) 実機/エミュレータ
     での起動確認自体(`adb install`→タップ→ログ確認)もこのパスでは
     未実施——ビルド成功の確認までに留まる。(c) 上記3.の`CAP_NET_RAW`
     懸念が実際にどう現れるか(`bind_device`が本当に`EPERM`を返すか、
     一部Android実装では緩和されているか)は未検証。(d) `NetworkBinder`
     が返す実際のインターフェース名(`wlan0`/`eth0`等)が、`rs-linkfusion`
     本体側の`NetworkInterface::show()`(今回パッチしたAndroid実装)が
     `/proc/net/dev`から取得する名前と一致するかどうかの突き合わせ確認も
     未実施。
  8. **本項目クローズ時の再検証(2026-08-05続き)**: デスクトップ側
     `cargo test --release`を改めて実行し、19件全green・回帰無しを
     再確認した(上記4.の主張の裏取り)。`android/`ビルド成果物
     (`app-debug.apk`等)自体の再ビルドはこのセッションでは行って
     いない——上記6.の検証結果(BUILD SUCCESSFUL)を追加検証なしで
     引き継いでいる点に留意。
  - 次にすべきこと: (1) root化済みAndroid実機(またはCAP_NET_RAW付与
    済みのカスタムROM/システムアプリ)での実際の複数インターフェース
    ボンディング動作確認(上記3.の懸念の実証・反証)、(2) 非root実機での
    「WiFiのみ」「USB-Ethernetのみ」それぞれ単一インターフェースでの
    `connect`/`serve`動作確認(こちらは`bind_device`の対象インターフェース
    が1つのみでも、そもそも`set_interface_filter`等で単一に絞る運用が
    現実的な代替策になりうる、要検討)、(3) `NetworkBinder`が検知した
    実際のインターフェース名と本体側列挙結果の突き合わせ、(4) 実機での
    `adb install`→起動→ログ確認、(5) GUI/フォアグラウンドサービス化・
    APK署名配布は引き続きスコープ外。

- **2026-08-03 前回エントリ「次にすべきこと(2)」(TUN層を持たない縮小
  スコープでのAndroid対応可否)に着手、ブロッカーを1段階先へ絞り込み**:
  1. **`tun-gateway`featureを新設**(既定ON、既存のデスクトップ挙動は
     無変更): `tun-rs`を`optional`依存にし、`mod tun_gateway`・
     `GatewayServe`/`GatewayConnect`サブコマンド・その実処理関数を
     `#[cfg(feature = "tun-gateway")]`で囲んだ。`cargo build
     --no-default-features`で`tun-rs`(=Android未対応の根本原因)を
     完全に外せるようになった。
  2. **`cargo ndk -t aarch64-linux-android build --release
     --no-default-features`を実行したところ、`tun-rs`起因のエラーは
     解消し、コンパイル自体は成功、リンク段階で別の新しいエラーに
     到達**: `aggligator-transport-tcp`(TUNを使わない`serve`/`connect`
     ボンディング自体が依存する、複数物理インターフェースを実際に
     束ねるための中核クレート)が`network-interface`crateへ
     **無条件に**依存しており、その`getifaddrs`/`freeifaddrs`
     (glibc/BSD API)呼び出しがAndroidのBionic libcにはリンクできず
     `undefined symbol`でリンク失敗する(`network-interface`v2.0.5の
     ソースを確認したところ`#[cfg(any(target_os = "android", target_os
     = "linux"))]`と明記されており、Android向けの分岐自体はあるが、
     実際にはBionic libcにこれらのシンボルが存在しないため落ちる、
     という`tun-rs`とは異なる種類の非互換)。
  3. **自前で`network-interface`を使っていた箇所
     (`src/speedtest.rs::detect_environment`、ネット速度測定時の
     インターフェース内訳記録用)は`/proc/net/dev`ベースの依存無し
     フォールバックに置き換え、`Cargo.toml`で`network-interface`
     自体を`[target.'cfg(not(target_os = "android"))'.dependencies]`
     へ移動した**——これでこちらの直接依存は解消したが、
     `aggligator-transport-tcp`側の依存はこちらの制御が及ばず
     残ったまま(上記2.のリンクエラーの直接原因)。
  4. **検証(実測)**: `cargo build`/`cargo test --release`(デフォルト
     features)19件全green、`cargo test --release
     --no-default-features`も19件全green(回帰無し)。
  5. **結論・スコープの絞り込み(正直な開示)**: 「Android対応が
     一切不可能」だった状態(`tun-rs`の`DeviceBuilder`が根本的に
     Android非対応)から、「`connect`/`serve`(TUN無し、単純な
     ポートフォワーディング・ボンディング)は`aggligator-transport-tcp`
     の`network-interface`依存というただ1点のみが障壁」という、
     はるかに狭い・対応しやすいブロッカーまで絞り込めた。
  - 次にすべきこと: (1) `aggligator-transport-tcp`側の
    `network-interface`依存を回避する方法の検討——選択肢は
    (a) `network-interface`のAndroid対応版へのアップグレード待ち/
    upstream側へのissue報告、(b) `aggligator-transport-tcp`を
    フォークして依存を差し替える(メンテナンスコスト増)、
    (c) Androidでは単一インターフェース(モバイル回線のみ)固定で
    妥協し、`aggligator-transport-tcp`自体を使わない薄い代替経路を
    Android専用に実装する、のいずれか。(2) 上記が解決すれば
    `connect`/`serve`のみのAndroidバイナリが実際に生成できるはずなので、
    それを実機で動作確認する。(3) 真のTUNベースのVPN機能(`gateway-*`)は
    引き続き`VpnService`+JNIの大規模な別プロジェクトが必要
    (変更なし)。

- **2026-08-01 Android NDKクロスビルド可否を実証(ユーザー指示「Android
  対応・複数ドメインバイナリ共有等の残りの横断バックログを行なう」、
  `open-raid-z/CLAUDE.md`の段階的着手方針「2. まだNDKクロスビルド自体を
  試していないリポジトリは、まずビルド可否の実証から着手する」に対応)**:
  `cargo ndk -t aarch64-linux-android build --release`を実際に実行した
  結果、**ビルドは失敗する**ことを確認した——`open-redmine`/`aruaru-db`
  のようにそのまま成功するケースとは異なる、正直な開示。
  1. **エラー内容**: `src/tun_gateway.rs`が`tun_rs::DeviceBuilder`
     (TUNインターフェースの作成・設定を行う高レベルAPI)を使っているが、
     `tun-rs`クレートの`src/platform/`配下には`linux`/`macos`/`windows`/
     `freebsd`/`openbsd`/`netbsd`/`apple`はあるが**`android`が無く**、
     `DeviceBuilder`自体がAndroidターゲットではコンパイルされない。
  2. **根本原因(`tun-rs`のソースを実際に読んで特定、推測ではない)**:
     Androidのセキュリティモデルは、一般アプリが`/dev/tun`のような
     TUNデバイスを直接オープン・作成することを許可しない——TUN機能を
     使うには`VpnService`(Android標準API)がユーザーに許可ダイアログを
     出した上でファイルディスクリプタ(FD)を払い出し、アプリはその
     FDを受け取って読み書きするだけ、という全く異なるモデルになる。
     `tun-rs`はこのAndroid固有のFD受け渡しモデルを`DeviceBuilder`
     (Linuxデスクトップ同様「自分でインターフェースを作る」前提のAPI)
     ではサポートしていない。
  3. **完全に不可能ではない(次回以降の設計課題として記録)**:
     `src/platform/unix/tun.rs`を読むと、`#[cfg(any(target_os = "linux",
     target_os = "android"))]`という低レベルAPI(既存のFDをラップする
     形の`Tun`構造体メソッド群)は実際に存在する——`DeviceBuilder`が
     使えないだけで、クレート自体がAndroidを一切想定していないわけでは
     ない。真にAndroid対応するには、(a) Kotlinネイティブアプリシェルで
     `VpnService`を実装しユーザー許可を得てFDを取得、(b) JNI経由で
     そのFDをRust側へ渡し、`tun_gateway.rs`を「`DeviceBuilder`で新規
     作成」ではなく「既存FDをラップする」経路に書き換える、という
     大掛かりな設計変更が必要——open-redmine/aruaru-db版のような
     「疎通確認+ブラウザ起動」の薄いシェルでは済まない規模。
  - 次にすべきこと: (1) `VpnService`+JNI FD受け渡しによる本格的な
    Android対応(規模の大きい別プロジェクトとして計画すべき)、
    (2) 上記が困難な場合、Android版は「PC側でボンディングしたトンネルへ
    モバイル回線からアクセスするだけのクライアント」等、TUN層を持たない
    縮小スコープでの対応可否を検討する。

- **2026-07-24 open-web-serverとの連携を実機検証(結論: 追加のコード変更は
  不要)**: `open-web-server`側からの要望「同一PCに両方インストールし、
  複数回線をボンディングした上でWebサーバーを動かす」シナリオを検証。
  `serve --bind 127.0.0.1:15900 --target 127.0.0.1:18099`(ボンディング
  受け口、転送先を`open-web-server`のポートに設定)+
  `connect --listen 127.0.0.1:15199 --remote 127.0.0.1 --remote-port
  15900`(ボンディング接続元)を実際に起動し、`curl
  http://127.0.0.1:15199/healthz`で`open-web-server`の`/healthz`へ複数回
  疎通(3回連続`200 ok`)を実TCPソケット経由で確認した。`aggligator`側の
  ログで実リンク確立、`open-web-server`側の`tracing`ログで実際に
  `GET /healthz status=200`のリクエストが届いていることも確認済み——
  モックではなく実プロセス3本での検証。**このリポジトリ側にも
  `open-web-server`側にも追加のコード変更は不要**(`serve`/`target`は
  単に既存のTCPサービスへ転送するだけで、対象アプリの種類を一切問わない
  設計のため)。
  **正直な限界**: 上記は`serve`/`connect`(TCPポートフォワードモード)
  での検証であり、ユーザーが想定する本命シナリオである`gateway-serve`/
  `gateway-connect`(TUN仮想アダプタ方式、OSレベルの全トラフィックを
  ボンディングする)は、Windowsで`wintun.dll`+管理者権限が必要なため、
  この開発環境(非管理者権限、`IsInRole(Administrator)=False`を確認済み)
  では実機検証できなかった。ただし設計上は同様に動作するはず
  (`open-web-server`は`OPEN_WEB_SERVER_BIND`でbindアドレスを外部注入
  するだけでネットワークインターフェースに関知しない)。**次回、管理者
  権限のある実機環境がある場合、`gateway-connect`で確立したTUN仮想
  アダプタのIP(既定`10.66.0.2`)に`OPEN_WEB_SERVER_BIND`を向けて同様の
  検証を行うこと**。

- **2026-07-23 GPUバックエンドの実装完了・実機検証・セキュリティ修正**:
  - `opencuda-directx`(DirectX 12 Compute)バックエンドを`AccelBackend::Gpu`
    に統合し、ChaCha20暗号化をGPUオフロード可能にした(`--accel gpu`で
    `serve`/`connect`/`gateway-serve`/`gateway-connect`全サブコマンドから
    選択可能)。
  - **訂正(このマシンでの実機検証結果に基づく)**: 一時、このHANDOFFに
    「GT730はDirectX 12非対応のためCPUフォールバックする」という誤った
    記述があったが、事実と異なる。**GT 730はDirectX 12に対応している
    (Feature Level 11_0)**——`open-cuda`側のセッションで
    `D3D12CreateDevice`の実機成功・DXGIアダプタ列挙での
    `"NVIDIA GeForce GT 730"`取得・ChaCha20/matmul/vector_addの実GPU
    ディスパッチとCPU参照実装との完全一致を複数のテストで検証済み
    (詳細は`open-cuda`側`CLAUDE.md`のHANDOFF参照)。GT730が対応しない
    のはDirectX 12の新しいFeature Level(12_x系、Ray Tracing等)であり、
    「DirectX 12非対応」という表現は誤り。
  - **実重大バグの発見・修正**: 当初のGPU実装は認証タグ(Poly1305)を
    計算しておらず、GPUバックエンド選択時に改ざん検知が効かない
    (`open()`が改ざんデータを受理してしまう)という実質的な脆弱性が
    あった。RFC 8439のAEAD構成(counter=0ブロックからPoly1305一時鍵を
    導出、実データはcounter=1から暗号化)をCPU側`poly1305`crateで
    実装し、GPU(ChaCha20部分)と組み合わせることで解消。この構成が
    `chacha20poly1305`crateの出力と完全一致することをテストで検証済み
    (`gpu_poly1305_construction_matches_chacha20poly1305_reference`)。
    GPUバックエンドでの改ざんフレーム拒否も実機で確認済み
    (`gpu_backend_tampered_frame_is_rejected_if_available`)。
  - Vulkanバックエンド追加は現時点で必須ではない(DirectX 12で
    GT730含め動作確認済みのため)。将来的にmacOS/Linux/Android等
    非Windows環境でGPU加速したい場合の選択肢として残る。

- **2026-07-23(続き) TUNゲートウェイ・QoS・速度測定・GUI・自動再接続を
  追加(ユーザー指示、複数回にわたる追加要望を反映)**:
  1. **TUN仮想アダプタ方式のフルVPNゲートウェイ**(`src/tun_gateway.rs`、
     `gateway-serve`/`gateway-connect`サブコマンド)。`tun-rs`クレート
     (Windows: `wintun.dll`、Linux: カーネルTUN)でIPパケット単位の
     捕捉・注入を行い、既存の`framed`(圧縮+暗号化)をパケット単位で
     再利用。IPフォワーディング/NAT・デフォルトルート切替は自動化せず
     手動設定前提と明記(README.md参照)——OS設定の無断書き換えを
     避けるため。
  2. **自動再接続(ユーザー指示「WANとLANとWiFiのミックス対応接続は
     システム側で変化があっても自動で接続状況確認、自動最適調整で、
     自動対応」)**: `run_gateway_connect`を無限ループ化し、
     `RS-SmartTCP::AdaptivePolicy::retry_backoff()`で再試行間隔を
     品質に応じて自動調整。個々の物理インターフェースの増減自体は
     `aggligator::TcpConnector::link_tags`が内部で10秒間隔で自動
     再走査するため、このループは「ボンディング接続そのものが完全に
     切断された場合」の再確立を担う。
  3. **QoS(HiEndオーディオ向け帯域制御、ユーザー指示)**: `src/qos.rs`。
     DNS応答(UDP/53)スヌーピングでNetflix/U-NEXT/YouTube/Qobuz等の
     IPを分類し、そのトラフィックだけトークンバケットで帯域制限
     (既定10Mbps)、それ以外は同時アクセスでも無制限。`--qos-config`
     で任意有効化(既定オフ)、`default`で内蔵プリセット。**開発中に
     実バグを発見・修正**: `RateLimiter::consume()`がバースト容量を
     超える単発リクエストで永久にハングする欠陥があり、`cargo test`
     が実際にハングして発覚(型チェックのみで「完了」としない方針が
     機能した具体例)。トークンの初期値をバースト容量分で満たす標準的な
     設計に修正し、ハング再現テストを追加して検証済み。
  4. **ネット速度測定・自動記録**(ユーザー指示「自動測定・自動記録」):
     `src/speedtest.rs`。M-Lab(`ndt7`プロトコル、Google検索の速度
     テストと同じ基盤、`ndt7-client`クレート)のみ自動化——gate02/
     osakagas等の非公式サイトはユーザー確認の上、自動化せず手動記録
     機能(`speedtest record-manual`)に留めた(利用規約違反・破損
     リスクを避けるため)。測定時のネットワーク環境(インターフェース
     数・有線/無線内訳)を自動検出して併記。`speedtest prune`で古い
     記録の確認付き一括削除。
  5. **GUI**(ユーザー指示「速度測定というボタンを押したら実行される
     ように」「押さなければ良いように」): `src/gui.rs`、`egui`/
     `eframe`(Tauriには依存しない、既存エコシステム方針を踏襲)。
     ボタン押下=同意、確認ダイアログなし。「自動測定」チェックボックス
     で1時間ごとの無人測定を有効化。`gui` Cargo feature(既定オン)。
  6. **検証**: `cargo build`/`cargo test`ともgreen(10件、新規5件は
     `qos.rs`)。**GUIの実機検証は限定的**: デバッグログで実GPU
     (NVIDIA GT 730)でのOpenGL 3.3コンテキスト生成・ウィンドウ作成
     成功を確認したが、この開発環境の画面キャプチャ制限により実際の
     見た目・ボタン操作の目視確認はできなかった(正直な限界)。
     TUNゲートウェイ・QoSは実TUNデバイス経由の実機検証は未実施
     (管理者権限・複数物理NICが無いサンドボックス環境の制約)。
  7. **GPU/NPUアクセラレーション調査**(ユーザー指示「open-cudaも
     活かせたら活かして」「DirectXのプラグインとして」): `open-cuda`
     を調査した結果、実際はVulkan Compute基盤でDirectXへの依存は
     無く、圧縮・暗号化カーネルも存在しないことが判明。ユーザーは
     DirectX版への仕切り直しを希望したが、`aruaru-llm`への影響も
     及ぶ大きな方針転換のため、**次回はopen-cuda専用セッションで
     着手する**方針とし、`open-cuda`側CLAUDE.mdへ引き継ぎメモを
     記録・push済み(このリポジトリ側でのGPUアクセラレーション実装は
     今回未着手のまま)。

- **2026-07-23 コアロジック実装・実機検証完了**: 前回セッションが
  リミット接近で中断していた`cargo build`/`cargo test`未検証状態を
  解消。
  1. `cargo build`成功(警告2件のみ、`AccelBackend`の未実装
     バリアント`Gpu`/`Npu`/`HardwareAccelerator`が未使用という
     dead_code警告——`open-web-server-wire::accel`と同じ設計上
     意図的な未使用のため実害なし)。
  2. `cargo test`で既存5件(accel 3件・framed 2件)全green。
  3. `main.rs`を新規実装。`clap`で`generate-key`/`serve`/`connect`の
     3サブコマンドを提供。`serve`は`aggligator_transport_tcp::simple::
     tcp_server`でボンディング接続を受け付けローカルターゲットへ
     `TcpStream::connect`、`connect`は`tokio::net::TcpListener`で
     ローカル待受し、接続ごとに`simple::tcp_connect`でボンディング
     接続を新規に張る。両者とも`tokio::io::split`+`tokio::try_join!`
     による双方向リレー(`relay()`関数、`framed::write_frame`/
     `read_frame`でボンディング側を圧縮+暗号化)。鍵は64桁hex文字列
     で`serve`/`connect`双方に手動で渡す設計(`generate-key`
     サブコマンドで生成)。
  4. **実機検証(型チェックのみで完了と報告しない方針の徹底)**:
     Python製ループ型echoサーバー(127.0.0.1:9402)を用意し、
     `rs-linkfusion serve --bind 127.0.0.1:9501 --target 127.0.0.1:9402`
     と`rs-linkfusion connect --listen 127.0.0.1:9601 --remote 127.0.0.1
     --remote-port 9501`を実際に起動、ループバック上で実際に
     `aggligator`のリンクテスト(ping計測含む)が完了することを
     デバッグログで確認した上で、connect側のローカルポート
     (127.0.0.1:9601)へPythonクライアントから400バイト送信し、
     serve側経由でechoサーバーへ到達・折り返され、connect側から
     送信時と**完全に一致する400バイトが実際に返ってくる**ことを
     実TCPソケットで確認(`received 400 bytes / match: True`)。
     圧縮+暗号化フレーム化・ボンディング接続経由の往復・復号+解凍が
     実際に機能することを実証した。
     **正直な限界**: この開発環境はループバック(単一の仮想
     インターフェース「Loopback Pseudo-Interface 1」)のみのため、
     複数物理NICでの真のマルチホーミング効果自体は検証できていない
     (`aggligator`側のログでも単一インターフェースのみが列挙されて
     いることを確認済み)。
  5. `README.md`/`PORTING.md`を新規作成(3点セットが揃った)。
  6. `install.sh`/`install.ps1`を`open-web-server`の既存パターンから
     移植(サービスはTCP転送方式のため、環境変数ではなく
     `ExecStart`のコマンドライン引数でserve/connectを切り替える形に
     調整)。
  7. `.github/workflows/release.yml`を`open-web-server`の既存パターンから
     移植(タグpushでLinux/Windows向けバイナリ自動ビルド)。
     タグpushによる実リリース動作自体は未検証(次回優先事項)。
  - 次にすべきこと: 上記「未実装・次回セッションで最優先すべきこと」
    節を参照。

## 関連プロジェクト

- [open-raid-z](https://github.com/aon-co-jp/open-raid-z) — 開発ルールの正本。
- [RS-SmartTCP](https://github.com/aon-co-jp/RS-SmartTCP) — ネットワーク品質適応制御の利用元。
- [open-web-server](https://github.com/aon-co-jp/open-web-server) — `accel.rs`/`mptcp_channel.rs`と同じ設計パターンの原型。install.sh/install.ps1/release.ymlの参照元。

## エコシステム全体マップ

同時並行開発の対象プロジェクト一覧・各リポジトリの現況は
[`open-raid-z`のCLAUDE.md](https://github.com/aon-co-jp/open-raid-z/blob/main/CLAUDE.md)
「関連プロジェクト」節を参照。

## HANDOFF追記(2026-07-31続き2) VPS実デプロイ完了

前項「次にすべきこと(1)」への対応。

1. VPS(`ssh conoha`)に`/root/rs-link-fusion`(このリポジトリ)・
   `/root/open-cuda`(空だったため`git clone`し直し)を配置、
   `cargo build --release`成功(依存の`RS-SmartTCP`・`open-cuda`は
   既にVPS上に存在)。
2. `rs-link-fusion-landing.service`(systemd)を新設し、
   `rs-linkfusion landing --bind 127.0.0.1:8600`を常駐化
   (`RS_LINKFUSION_POWER_PROFILE=power-saving`)。
3. `/root/open-web-server/domains.toml`に`easy-web.tokyo`+
   `path_prefix=/rs-link-fusion`・`/rs-link-fusion/demo`
   (いずれも同一バックエンド`127.0.0.1:8600`へのエイリアス、
   `open-redmine`の`/demo`と同じ設計)を追記、`open-web-server`を
   再起動して反映。
4. **実機検証**: `curl https://easy-web.tokyo/rs-link-fusion/`→`200`・
   `<title>RS-Link-Fusion</title>`、`curl https://easy-web.tokyo/
   rs-link-fusion/demo`→`200`をいずれも実際のHTTPS経由で確認した。
5. **正直な開示**: デモ環境は本番と同一バックエンドへのエイリアスの
   ままで、独立したデータセットは無い(`open-redmine`のデモ環境と
   同じ制約)。このバイナリ自体はまだ`serve`/`connect`/`gateway-*`の
   実運用(ボンディング本体)をVPS上で稼働させていない
   ——今回デプロイしたのは`landing`サブコマンド(ダウンロード案内
   ページ)のみ。
  - 次にすべきこと: (1) Android対応(`open-raid-z/CLAUDE.md`の
    エコシステム横断優先方針に従う)、(2) `low_memory`単体の実メモリ
    削減実装、(3) ボンディング本体機能の実運用(GitHub Releasesでの
    バイナリ配布、実ユーザーによるserve/connect運用開始)。

## HANDOFF追記(2026-07-31続き) リポジトリ改名+電源プロファイル選択機能を実装+GitHub管理/デモページ設置

ユーザー指示の連続対応:「RS-LinkFusion→RS-Link-Fusion/rs-link-fusionへ
改名し、easy-web.tokyo/rs-link-fusionで管理・/demoへリンク」「電源
プロファイル選択機能の実装(選択制で省メモリ+省電力、常時電源も選択
可能)」「上記は同時には選択出来ない様にして」→「省電力+省メモリは
選択可能」→「省メモリ+常時電源接続も選択可能」(排他ルールを段階的に
確定)。

1. **リポジトリ改名(完了)**: GitHub API(`PATCH /repos/{owner}/{repo}`)
   経由で`aon-co-jp/RS-LinkFusion`→`aon-co-jp/rs-link-fusion`へ実際に
   リネーム。ローカルフォルダも`F:\runo\RS-LinkFusion`→
   `F:\runo\rs-link-fusion`へ変更、`git remote set-url`で追従。
   表示名は`RS-Link-Fusion`(README/CLAUDE.md見出し)。**正直な開示**:
   内部のcrate名・バイナリ名(`rs-linkfusion`、ハイフン無し)は既存の
   ビルドスクリプト・systemdサービス名への影響を避けるため据え置いた
   (README.mdに明記済み)。
2. **電源プロファイル選択機能(実装完了)**: `src/main.rs`に`PowerProfile`
   構造体(`power_saving`/`low_memory`/`always_on`の3bool)を新設。
   **排他ルール**(ユーザー指示を時系列で整理した最終形):
   - `power_saving`と`always_on`は同時指定不可(CPU使用率について
     正反対の方針のため)。
   - `low_memory`は独立した軸であり、`power_saving`・`always_on`の
     いずれとも併用可能。
   `--power-profile`(env: `RS_LINKFUSION_POWER_PROFILE`)にカンマ区切り
   で指定(例: `power-saving,low-memory`、`low-memory,always-on`)。
   **実効的な差分**(正直な開示、現時点で実装済みの範囲):
   - スレッド数: `always_on`が有効なら`tokio::runtime::Builder::
     new_multi_thread`(全論理コア)、無効なら`new_current_thread`
     (シングルスレッド)——`always_on`のみで判定し、`low_memory`との
     併用時に「フル性能」と「シングルスレッド」が矛盾しないようにした。
   - アクセラレータ: `always_on`が有効かつ`--accel`が既定値`cpu`のまま
     (ユーザーが明示的に指定していない)場合のみ`gpu`へ自動アップ
     グレード(`effective_accel()`)。ユーザーが明示的に`--accel cpu`を
     指定した場合はそちらを尊重する。
   - `low_memory`単体でのメモリ確保量削減(バッファ/キャッシュサイズの
     実際の調整)は**まだ未実装**——現状はランタイムのスレッド数決定
     への関与とログ出力のみが実効的な差分(次回課題として正直に開示)。
   - NPU自体(GPU以外のハードウェアアクセラレータ)は`accel.rs`の
     `AccelBackend::Npu`がプレースホルダのまま(既存の未実装項目、
     今回のスコープでは変更していない)。
3. **`install.sh`/`install.ps1`**: インストール時に5択(省電力/省メモリ/
   両方併用/常時電源接続/省メモリ+常時電源接続)のプロンプトを追加し、
   選択結果をLinuxはsystemdサービスの`Environment=`、Windowsは
   マシン環境変数(`[Environment]::SetEnvironmentVariable`、`Machine`
   スコープ)として設定する。Windows版は`New-Service`案内コマンドにも
   `--power-profile`明示指定を追加(サービスが環境変数を引き継がない
   場合への対策)。
4. **ランディングページ(`static/landing.html`)を新設**: ダウンロード
   リンク・電源プロファイルの説明(チェックボックス3つ、JSで
   「省電力」⇔「常時電源接続」の排他制御のみ実装、「省メモリ」は
   両方と併用可能なチェックボックスのまま)・機能概要(複数WAN/LAN/
   WiFi混在ボンディング・自動フェイルオーバー・open-raid-z/aruaru-db
   連携・open-directx/open-cuda連携)を日英併記で掲載。デモ環境
   (`/rs-link-fusion/demo`)への案内リンクも設置(open-redmineと同じ
   パターン)。**正直な開示**: このHTMLを実際に配信する軽量HTTPサーバー
   サブコマンド自体はまだ実装していない(このバイナリは元々CLIツールで
   あり、Webサーバー機能を持たない)——次回、`static/landing.html`を
   配信する最小限のHTTPサーバー(新規重量級依存を避け、既存の`tokio`
   のみで実装予定)を追加し、`open-web-server`の`domains.toml`へ
   `easy-web.tokyo`+`path_prefix=/rs-link-fusion`のテナント登録を行う
   必要がある。VPSへの実デプロイは**この理由により今回は未完了**。
5. **検証**: 新規テスト7件(`power_profile_tests`——併用可否・排他
   判定・スレッド数決定・アクセラレータ自動アップグレードの条件分岐を
   すべて実際の`PowerProfile::parse`/`effective_accel`呼び出しで確認)。
   `cargo test`**18件全green**(既存11件+新規7件、回帰なし)。
   `cargo build --release`成功(既存警告のみ、新規警告なし)。実バイナリ
   で`--power-profile power-saving,low-memory`・`always-on`・
   `low-memory,always-on`・`power-saving,always-on`(排他エラーになる
   ことを含む)を実際に実行し、ログ出力(シングル/マルチスレッド
   ランタイムの選択)が意図通りであることを確認した。
6. **Android/iOS対応の選択制インストーラー化(ユーザー指示「インストール
   ラーは、Windows、LINUX、Androidはスマホとタブレット選択制にして」)
   ——正直な開示、未着手**: 既存のHANDOFF記載通り、Android版は
   「Rust+Android NDKでのクロスビルドは技術的に可能性ありと考えられる
   が、ツールチェーン未確認・未着手」の計画段階のまま変わっていない。
   「スマホ/タブレット選択制」を実現するには、まずAndroidアプリシェル
   自体(APK化・署名・フォアグラウンドサービス化)が必要——これは
   インストーラーの選択肢を増やす以前の、より大きな未着手の前提作業
   であるため、今回はインストーラー側の変更は行わなかった。
   - 次にすべきこと: (1) 上記(4)の軽量HTTPサーバー実装+VPSデプロイ、
     (2) Android NDKクロスビルドの実証実験、(3) `low_memory`単体での
     実メモリ削減(バッファ/キャッシュサイズ調整)の実装、(4) NPU
     アクセラレータ本体の実装(現状プレースホルダ)。
