# RS-Link-Fusion

(旧名`RS-LinkFusion`→2026-07-31に`rs-link-fusion`〈GitHubリポジトリ名〉/
`RS-Link-Fusion`〈表示名〉へ改名。内部のcrate名・バイナリ名
〈`rs-linkfusion`〉は既存のインストール環境・ビルドスクリプトへの影響を
避けるため据え置き——正直な開示)

**開発開始日: 2026-07-23**(このリポジトリのGitHub作成日)

## Deployment / 運用先

- **Production/admin (本番/管理者用)**: <https://easy-web.tokyo/rs-link-fusion/>
- **Demo (デモ用)**: <https://easy-web.tokyo/rs-link-fusion/demo>

複数のWAN/LAN/WiFi(古い規格〜WiFi 7まで、OSがネットワークインター
フェースとして認識するものはすべて対象)を1つの論理接続へ束ね
(ボンディング)、通信の高速化・安定化を実現するアプリ。ポート単位の
軽量トンネル(`serve`/`connect`)と、PC上のあらゆる通信を束ねる
TUN仮想アダプタ方式のフルVPNゲートウェイ(`gateway-serve`/
`gateway-connect`)の2方式を提供する。インストーラー付きWindows/
Linux版として配布予定。

## 導入前におすすめすること

**RS-LinkFusion導入前に、以下いずれかで現在のネット速度を測定し、
記録を取っておかれることをお勧め致します**(導入後との比較用)。

- [Google検索の速度テスト](https://www.google.com/search?q=%E3%83%8D%E3%83%83%E3%83%88%E9%80%9F%E5%BA%A6%E6%B8%AC%E5%AE%9A)(M-Lab/ndt7ベース)
- [gate02 速度測定](https://speedtest.gate02.ne.jp/)
- [osakagas speedcheck](https://speedcheck.osakagas.co.jp/#!/?=true)

このうちM-Lab(Google検索の速度テストと同じ基盤)は`rs-linkfusion
speedtest run`で自動測定・自動記録できる(下記「ネット速度測定」参照)。
gate02・osakagasは非公式サイトのため自動化しておらず、手動で開いて
読み取った値を`rs-linkfusion speedtest record-manual`で同じ履歴へ
記録できる。

## これは何か

- **複数インターフェースのボンディング**: [`aggligator`](https://docs.rs/aggligator)
  (`aggligator-transport-tcp`)を採用。`TcpConnector::set_multi_interface(true)`
  (デフォルトで有効、Android以外)が、ローカルマシンが持つ**全ネット
  ワークインターフェース**(LAN・WiFi・複数WAN等、OSが認識するもの
  すべて)を自動列挙し、各インターフェース×各サーバーIPの組み合わせ
  ごとに個別のTCPリンクを確立、それらを1つの論理接続へ束ねる。
  「LAN」「WiFi」「WAN」を区別せず、古いWiFi規格〜WiFi 7まで、OSが
  IPを持つインターフェースとして認識してさえいればそのまま対象になる
  (WiFi規格自体の違いはOS/NICドライバ層で吸収される)。
- **自動再接続・自動最適化**: システム側でインターフェース構成が
  変化しても(LANケーブル抜き差し・WiFi接続/切断等)、`aggligator`が
  10秒間隔で自動的に再走査し追従する。ボンディング接続そのものが
  完全に切断された場合も、`gateway-connect`が[RS-SmartTCP](https://github.com/aon-co-jp/RS-SmartTCP)
  の適応バックオフ(実測RTT/ジッターに応じてFast/Slow切替)で自動的に
  再接続を試み続ける。
- **CPU/GPU/NPU/専用ハードウェアアクセラレータの抽象化**: `AccelBackend`
  列挙型(`Cpu`/`Gpu`/`Npu`/`HardwareAccelerator`)。`Cpu`は`flate2`圧縮+
  ChaCha20-Poly1305暗号化。`Gpu`(`gpu` feature、Windows専用)は
  [open-cuda](https://github.com/aon-co-jp/open-cuda)の`opencuda-directx`
  (DirectX 12 Compute)経由でChaCha20暗号化のみGPU実行し、認証タグ
  (Poly1305)はCPU側で計算してAEAD全体の改ざん耐性をCPUバックエンドと
  同等に保つ(`accel.rs`参照)。`--accel cpu`/`--accel gpu`で選択可能、
  GPU初期化に失敗した場合は安全に`Cpu`へフォールバックする。`Npu`/
  `HardwareAccelerator`は未実装の拡張点。
- **トンネル方式**: `[len:u32 LE][圧縮+暗号化済みペイロード]`という
  長さプレフィクスフレームで、ボンディング接続上にトラフィックを流す。
  `serve`/`connect`は固定1アドレスへのポート転送、`gateway-serve`/
  `gateway-connect`はTUN仮想アダプタでIPパケット単位のフルVPN。
- **QoS(HiEndオーディオ向け帯域制御、オプトイン)**: 動画配信/VOD/
  音楽配信サービス(Netflix・U-NEXT・YouTube・Qobuz等)向けの通信だけ
  帯域上限(既定10Mbps)をかけ、それ以外のダウンロード/アップロードは
  同時アクセスでもボンディング接続の実効速度まで無制限、という2層
  構成を選択制で提供する(`qos.rs`)。
- **ネット速度測定・自動記録**: [M-Lab](https://www.measurementlab.net/)
  (`ndt7`プロトコル、Google検索の速度テストと同じ基盤)で速度測定し、
  測定時点のネットワーク環境(インターフェース数・有線/無線の内訳)と
  併せてJSONL形式で記録する。`speedtest watch`で確認なしの定期自動
  測定も可能。
- **GUI(「速度測定」ボタン)**: `egui`/`eframe`製の最小限のウィンドウ
  (Tauriには依存しない、既存のエコシステム方針を踏襲)。ボタンを押す
  ことが同意——押さなければ測定は一切実行されない。

## 使用例(ポート転送モード)

```bash
# 鍵を1つ生成し、serve側・connect側で同じ値を使う
rs-linkfusion generate-key
# => 64桁のhex文字列

# リモート側(実サービスがあるマシン): ボンディング接続を受け付け、
# ローカルの実サービス(例: 127.0.0.1:8080)へリバースプロキシする
rs-linkfusion serve --bind 0.0.0.0:5900 --target 127.0.0.1:8080 --key <上記の鍵>

# ローカル側: ローカルポート(例: 127.0.0.1:8080)で待ち受け、
# serve側のボンディング接続へ転送する
rs-linkfusion connect --listen 127.0.0.1:8080 --remote <serve側のホスト名/IP> --remote-port 5900 --key <同じ鍵>
```

> **`open-web-server`との連携について(2026-07-24、実機検証済み)**:
> `--target`を`open-web-server`のbindアドレス(例:
> `OPEN_WEB_SERVER_BIND=127.0.0.1:18099`)に向けるだけで、追加のコード
> 変更なしにボンディング経由でWebサーバーへ到達できることを、実プロセス
> 3本(`open-web-server`+`rs-linkfusion serve`+`rs-linkfusion connect`)
> でのcurl疎通により確認済み。詳細は本リポジトリの`CLAUDE.md`
> (2026-07-24 HANDOFF)、および`open-web-server/PORTING.md` §4.12参照。

## 使用例(TUNゲートウェイ・フルVPNモード)

管理者権限が必要。Windowsでは[wintun.dll](https://wintun.net/)を
実行ファイルと同じディレクトリに配置すること。

```bash
# リモート側(典型的にはLinux VPS)
sudo rs-linkfusion gateway-serve --bind 0.0.0.0:5900 --key <鍵>
# QoSプリセット(動画/音楽配信を10Mbpsへ制限)を有効にする場合:
sudo rs-linkfusion gateway-serve --bind 0.0.0.0:5900 --key <鍵> --qos-config default

# ローカル側(Windows、管理者権限のPowerShellで)
rs-linkfusion gateway-connect --remote <serve側のIP> --remote-port 5900 --key <同じ鍵>
```

**正直な開示**: TUN作成後のIPフォワーディング/NAT(serve側、Linux)・
デフォルトルートのTUN経由への切り替え(connect側)は、このアプリ自身
では自動化していない(誤設定時の影響が大きいため、手動設定を前提と
する)。serve側の例(Linux、要root):

```bash
sysctl -w net.ipv4.ip_forward=1
iptables -t nat -A POSTROUTING -o eth0 -j MASQUERADE  # eth0は実際のWANインターフェース名に置き換え
```

connect側(Windows)でデフォルトルートをTUN経由に切り替える例:

```powershell
route add 0.0.0.0 mask 0.0.0.0 10.66.0.1 metric 1
```

## ネット速度測定

```bash
# 1回測定(M-Lab、対話的に同意確認)
rs-linkfusion speedtest run --label baseline

# 確認なしで1回測定(スクリプト等から)
rs-linkfusion speedtest run --label accelerated --yes

# 1時間ごとに確認なしで自動測定・自動記録し続ける(Ctrl+Cで終了)
rs-linkfusion speedtest watch --interval-minutes 60

# gate02/osakagas等、非公式サイトを手動で開いて読み取った値を記録
rs-linkfusion speedtest record-manual --source gate02 --download-mbps 350 --upload-mbps 120

# 履歴を表示
rs-linkfusion speedtest history

# 90日より古い記録を確認のうえまとめて削除
rs-linkfusion speedtest prune --older-than-days 90
```

## GPUアクセラレーション(`--accel gpu`、Windows専用)

暗号化(ChaCha20部分)を[open-cuda](https://github.com/aon-co-jp/open-cuda)の
`opencuda-directx`(DirectX 12 Compute)へオフロードできる。認証タグ
(Poly1305)は常にCPU側で計算するため、CPUバックエンドと同等の改ざん
耐性を持つ(`accel.rs`参照)。

```bash
cargo build --features gpu
```

DXILシェーダー(`chacha20.dxil`)はビルド成果物のためこのリポジトリには
含まれない。`open-cuda`側でコンパイルし、実行ファイルと同じディレクトリの
`shaders/chacha20.dxil`へ配置する(`install.ps1`は同梱時に自動コピーする):

```powershell
cd ..\open-cuda
.\tools\compile-dx12-shaders.ps1
Copy-Item crates\opencuda-directx\shaders\chacha20.dxil ..\RS-LinkFusion\shaders\ -Force
```

```bash
rs-linkfusion serve --bind 0.0.0.0:5900 --target 127.0.0.1:8080 --key <鍵> --accel gpu
```

GPU初期化やDXILシェーダーの読み込みに失敗した場合は、警告ログを出した
うえで安全に`Cpu`へフォールバックする。

## GUI

```bash
rs-linkfusion gui
```

「速度測定」ボタンを押すと測定・記録が実行される(押さなければ何も
起きない)。「自動測定」チェックボックスで1時間ごとの無人測定・記録
を有効化できる。

## 正直な開示

- 個々の物理リンク単位の内訳ではなく、ボンディングされた論理接続
  全体の実効品質という単純化を採用している(`quality.rs`)。
- **GPUアクセラレーション(`--accel gpu`)は実装済み・実機検証済み**
  (`open-cuda`の`opencuda-directx`、DirectX 12 Compute)。このマシンの
  NVIDIA GeForce GT 730(Kepler世代、DirectX 12 Feature Level 11_0
  対応——「DirectX 12非対応」ではない)で、実際のGPUディスパッチと
  CPU参照実装(`chacha20poly1305`crate)との出力完全一致を検証済み
  (`accel.rs`のテスト参照)。**ただし**トンネル1フレームは小サイズ
  (MTU程度)のため、Host↔Device間の転送オーバーヘッドがGPU側の
  演算優位性を相殺し実利益が出ない可能性がある、という性能上の懸念
  は未検証のまま(正しさは検証済みだが、速度面でCPUより有利かどうか
  のベンチマークは今後の課題)。NPU/専用ハードウェアは未実装。
- **QoSのサービス分類はDNS応答スヌーピングによるベストエフォート**。
  CDN・エニーキャストIPは複数サービスで共有されることがあるため、
  分類の精度は完全ではない(`qos.rs`参照)。
- **TUNゲートウェイは複数クライアント同時接続を想定していない**
  (1つのTUNデバイスに対し単一クライアント前提)。
- macOS/Android/iOS/スマートTV対応は計画のみ(詳細は`CLAUDE.md`参照)。
- **この開発環境では管理者権限・複数物理NIC・実際のTUNドライバでの
  実機検証ができていない**(サンドボックス環境の制約)。ループバック
  上でのポート転送モードの実データ往復は実機検証済み。GUIはウィンドウ
  作成・実GPU(OpenGL)コンテキスト生成の成功をログで確認済みだが、
  この環境の画面キャプチャ制限により見た目の目視確認はできていない。

## 対応プラットフォーム

| プラットフォーム | 状況 |
|---|---|
| Windows | 主要ターゲット。`install.ps1`で導入可能。TUNゲートウェイには`wintun.dll`が必要 |
| Linux | 主要ターゲット。`install.sh`で導入可能 |
| macOS/Android/iOS/スマートTV | 計画のみ(詳細は`CLAUDE.md`参照) |

## このエコシステムでの関連

- [RS-SmartTCP](https://github.com/aon-co-jp/RS-SmartTCP) — ネットワーク
  品質適応制御・自動再接続バックオフの利用元。
- [open-web-server](https://github.com/aon-co-jp/open-web-server) —
  `accel.rs`/`aggligator`利用パターンの原型
  (`open-web-server-wire::accel`/`mptcp_channel`)。
- [open-cuda](https://github.com/aon-co-jp/open-cuda) — GPU抽象化基盤
  (現状Vulkan Compute、DirectX版への方針転換をユーザーが検討中、
  詳細は同リポジトリのCLAUDE.md HANDOFF参照)。

## ビルド・テスト

```bash
cargo build
cargo test
```

GUIを含めない場合(`gui` featureは既定で有効):

```bash
cargo build --no-default-features
```

## ライセンス

Apache-2.0

## API 方針: REST 全廃・gRPC / Connect へ (2026-11-04)

### 日本語
- **REST は全廃する。新しい API は REST で作らない。** ブラウザや `curl` から呼ぶ API は **Connect の単純呼び出し**(`POST /<パッケージ>.<Service>/<Method>`、`content-type: application/json`、本文は JSON)、サービス間は **gRPC / gRPC-web**(RPoem の管理API `open_runo.admin.v1.Admin` と同じ流儀)。ストリーミングは、必要になってから。
- **既存の REST は、触るときに順に移す。** 移したら旧 REST の経路は消す。移行が終わるまで残るが、**新規には追加しない**。
- **JSON は RS-JSON に統一**する(読み込みは `from_str_secure` / `parse_secure` でサイズ・深さ・重複キーの上限つき、書き出しは `rust_json::fast`)。ブラウザの WASM に `prost` は入れない(サイズが増えるため)。
- **例外(自分たちの API ではなく、相手の規格が決めているもの)**: ACME、OAuth/OIDC のコールバック、第三者(GitHub など)からの Webhook、ヘルスチェック、静的ファイル配信、OTLP など。これらは規格どおりの HTTP を受ける。
- 参考実装: `RPoem`(`open_runo.admin.v1.Admin`)、`open-easy-web`(`server/src/connect.rs`、`proto/open_easy_web/admin/v1/admin.proto`)。
- ※ このリポジトリのコードが既に方針どおりかどうかは、各リポジトリの現状による。この節は方針の記録であり、コードの変更を意味しない。

### English
- **REST is being abolished. New APIs are not built as REST.** APIs called from browsers or `curl` use **Connect unary calls** (`POST /<package>.<Service>/<Method>`, `content-type: application/json`, JSON body); service-to-service calls use **gRPC / gRPC-web** (the same style as RPoem's admin API `open_runo.admin.v1.Admin`). Streaming only when it is actually needed.
- **Existing REST endpoints are migrated one by one, when they are touched.** The old REST route is removed once migrated. Until migration is finished they remain, but **no new ones are added**.
- **JSON is unified on RS-JSON**: reading with `from_str_secure` / `parse_secure` (size, depth and duplicate-key limits), writing with `rust_json::fast`. `prost` is not put into browser WASM (it would grow the binary).
- **Exceptions (shapes dictated by someone else's standard, not our own API)**: ACME, OAuth/OIDC callbacks, webhooks from third parties (e.g. GitHub), health checks, static file serving, OTLP, and so on. These accept plain HTTP as the standard requires.
- Reference implementations: `RPoem` (`open_runo.admin.v1.Admin`) and `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Note: whether the code in a given repository already follows this policy depends on that repository. This section records the policy; it does not mean the code has been changed.

### Deutsch (KI-Übersetzung, redaktionell überarbeitet; noch nicht von Muttersprachlern geprüft)
- **REST wird vollständig abgeschafft; neue APIs entstehen nicht mehr als REST.** APIs, die aus dem Browser oder per `curl` aufgerufen werden, verwenden **unäre Connect-Aufrufe** (`POST /<Paket>.<Service>/<Methode>`, `content-type: application/json`, JSON-Body). Zwischen Diensten kommen **gRPC bzw. gRPC-web** zum Einsatz, im selben Stil wie die Admin-API von RPoem (`open_runo.admin.v1.Admin`). Streaming wird erst ergänzt, wenn es tatsächlich gebraucht wird.
- **Bestehende REST-Endpunkte werden schrittweise umgestellt, und zwar immer dann, wenn man ohnehin an ihnen arbeitet.** Sobald ein Endpunkt umgestellt ist, wird die alte REST-Route entfernt. Bis zum Abschluss der Migration bleiben die übrigen bestehen, es werden aber **keine neuen mehr hinzugefügt**.
- **JSON wird einheitlich über RS-JSON verarbeitet**: Gelesen wird mit `from_str_secure` bzw. `parse_secure` (mit Obergrenzen für Größe, Verschachtelungstiefe und doppelte Schlüssel), geschrieben mit `rust_json::fast`. `prost` wird nicht in das Browser-WASM aufgenommen, da es die Datei vergrößern würde.
- **Ausnahmen (Schnittstellen, deren Form ein fremder Standard vorgibt und die nicht zu unserer eigenen API gehören)**: ACME, OAuth/OIDC-Callbacks, Webhooks von Drittanbietern (z. B. GitHub), Health-Checks, die Auslieferung statischer Dateien, OTLP und Ähnliches. Diese nehmen, wie vom jeweiligen Standard verlangt, gewöhnliches HTTP entgegen.
- Referenzimplementierungen: `RPoem` (`open_runo.admin.v1.Admin`) und `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Hinweis: Ob der Code eines einzelnen Repositorys diese Richtlinie bereits erfüllt, hängt vom jeweiligen Repository ab. Dieser Abschnitt dokumentiert lediglich die Richtlinie; er besagt nicht, dass der Code schon angepasst wurde.

### Русский (перевод выполнен с помощью ИИ и отредактирован; носителями языка пока не проверен)
- **От REST мы полностью отказываемся; новые API больше не создаются в стиле REST.** API, к которым обращаются из браузера или через `curl`, используют **унарные вызовы Connect** (`POST /<пакет>.<Service>/<Method>`, `content-type: application/json`, тело запроса — JSON). Для взаимодействия между сервисами применяются **gRPC и gRPC-web** — в том же стиле, что и административный API RPoem (`open_runo.admin.v1.Admin`). Потоковая передача данных добавляется лишь тогда, когда в ней действительно возникает необходимость.
- **Существующие REST-эндпоинты переводятся поэтапно — по мере того, как с ними приходится работать.** После перевода старый REST-маршрут удаляется. Пока миграция не завершена, оставшиеся эндпоинты сохраняются, но **новые не добавляются**.
- **Работа с JSON унифицирована на основе RS-JSON**: чтение — через `from_str_secure` или `parse_secure` (с ограничениями на размер, глубину вложенности и повторяющиеся ключи), запись — через `rust_json::fast`. `prost` в браузерный WASM не включается, поскольку это увеличило бы размер файла.
- **Исключения (интерфейсы, форму которых определяет чужой стандарт, а не наш собственный API)**: ACME, обратные вызовы OAuth/OIDC, вебхуки сторонних сервисов (например, GitHub), проверки работоспособности, раздача статических файлов, OTLP и тому подобное. Такие интерфейсы принимают обычный HTTP, как того требует соответствующий стандарт.
- Эталонные реализации: `RPoem` (`open_runo.admin.v1.Admin`) и `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Примечание: соответствует ли код конкретного репозитория этой политике, зависит от самого репозитория. Данный раздел лишь фиксирует политику и не означает, что код уже изменён.

## API 方針の補足: 用語の整理と使い分け (2026-11-04)

### 日本語
**1つに統一するものではなく、別の層の話です。** 「REST の代わり」になるのは Connect・gRPC・GraphQL だけで、WASM は画面を作る技術です。

| 用語 | 何か | REST の代わりか |
|---|---|---|
| REST | URL と HTTP メソッドで操作を表す API の作り方 | 全廃する側 |
| **Connect** | `POST /サービス名/メソッド名` に JSON を送る形式。`curl` でもブラウザの `fetch` でも呼べる | はい(**採用**) |
| **gRPC** | 同じ「サービス/メソッド」の考え方を、バイナリ(protobuf)と HTTP/2 で行う形式。速く、ストリーミングも得意 | はい(**サービス同士に採用**) |
| gRPC-web | gRPC のブラウザ向けの変形版 | はい(必要なら) |
| GraphQL | 欲しい項目を問い合わせる形式 | 一部だけ(**問い合わせ(読み取り)専用**) |
| WASM | Rust をブラウザの中で動かす仕組み | いいえ(画面を作る技術) |

- **ブラウザの画面や `curl` から呼ぶ API は Connect**(POST、JSON を RS-JSON で読み書き)。ブラウザの WASM は素の gRPC を話せず、Connect なら `fetch` のままで、`prost` を足さずに済む(画面のサイズも増えない)。
- **サービス同士の通信(サーバー間、重い処理、ストリーミング)は gRPC。** 多くは Connect で足りるので、ストリーミングが必要になった所だけ gRPC に上げる。
- **GraphQL は、まとめて問い合わせたいときの読み取り専用。** 書き込み(操作)には使わない。
- **WASM は画面を作る技術として、そのまま使う。** 画面から呼ぶ API が Connect になっただけ。
- 外部の規格が形を決めるもの(ACME、OAuth のコールバック、第三者の Webhook、ヘルスチェック、静的ファイル、OTLP)は、規格どおりの HTTP のまま。
- **迷ったら Connect。**

### English
**These are not alternatives to unify; they sit at different layers.** Only Connect, gRPC and GraphQL can replace REST; WASM is a way to build the UI.

| Term | What it is | Replaces REST? |
|---|---|---|
| REST | API style that expresses operations through URLs and HTTP methods | The thing being abolished |
| **Connect** | `POST /Service/Method` with a JSON body; callable from `curl` and from a browser `fetch` | Yes (**adopted**) |
| **gRPC** | The same service/method idea over binary (protobuf) and HTTP/2; fast, good at streaming | Yes (**for service-to-service**) |
| gRPC-web | A browser-friendly variant of gRPC | Yes (if needed) |
| GraphQL | A query language: ask for exactly the fields you want | Partly (**read-only queries**) |
| WASM | Runs Rust inside the browser | No (a UI technology) |

- **APIs called from browser screens or `curl` use Connect** (POST, JSON read and written with RS-JSON). Browser WASM cannot speak plain gRPC; with Connect it stays a `fetch`, and `prost` is not needed (the binary does not grow).
- **Service-to-service calls (server to server, heavy work, streaming) use gRPC.** Connect is enough in most places; move a call to gRPC only where streaming is actually needed.
- **GraphQL is read-only**, for queries that gather several pieces of data. It is not used for writes (operations).
- **WASM stays as the way to build the UI.** The API it calls simply became Connect.
- Interfaces whose shape an outside standard dictates (ACME, OAuth callbacks, third-party webhooks, health checks, static files, OTLP) stay plain HTTP.
- **When in doubt, use Connect.**

### Deutsch (KI-Übersetzung, sprachlich überarbeitet; noch nicht von Muttersprachlern geprüft)
**Es geht nicht darum, eine einzige Technik auszuwählen – die Begriffe liegen auf unterschiedlichen Ebenen.** Als Ersatz für REST kommen nur Connect, gRPC und GraphQL in Frage; WASM dagegen ist eine Technik, mit der die Benutzeroberfläche gebaut wird.

| Begriff | Was ist das? | Ersatz für REST? |
|---|---|---|
| REST | Ein API-Stil, bei dem Operationen über URLs und HTTP-Methoden ausgedrückt werden | wird abgeschafft |
| **Connect** | `POST /Dienst/Methode` mit JSON im Body; aufrufbar mit `curl` ebenso wie mit `fetch` im Browser | ja (**gewählt**) |
| **gRPC** | Dasselbe Prinzip aus Dienst und Methode, aber über binäres protobuf und HTTP/2; schnell und gut für Streaming geeignet | ja (**zwischen Diensten**) |
| gRPC-web | Eine für den Browser angepasste Variante von gRPC | ja (bei Bedarf) |
| GraphQL | Abfragesprache, mit der man genau die benötigten Felder anfordert | teilweise (**nur für lesende Abfragen**) |
| WASM | Führt Rust-Code direkt im Browser aus | nein (Technik für die Oberfläche) |

- **APIs, die von Browseroberflächen oder per `curl` aufgerufen werden, verwenden Connect** (POST; JSON wird mit RS-JSON gelesen und geschrieben). WASM im Browser kann kein reines gRPC sprechen. Mit Connect bleibt es bei einem gewöhnlichen `fetch`, und `prost` ist nicht nötig – die WASM-Datei wird dadurch nicht größer.
- **Für die Kommunikation zwischen Diensten (Server zu Server, rechenintensive Aufgaben, Streaming) wird gRPC eingesetzt.** In den meisten Fällen reicht Connect aus; auf gRPC wird nur dort umgestellt, wo Streaming tatsächlich gebraucht wird.
- **GraphQL dient ausschließlich lesenden Abfragen**, wenn mehrere Informationen gebündelt abgefragt werden sollen. Für Schreibzugriffe (Operationen) wird es nicht verwendet.
- **WASM bleibt die Technik für die Oberfläche.** Geändert hat sich lediglich, dass die aufgerufene API nun auf Connect beruht.
- Schnittstellen, deren Form durch einen externen Standard festgelegt ist (ACME, OAuth-Callbacks, Webhooks von Drittanbietern, Health-Checks, statische Dateien, OTLP), bleiben gewöhnliches HTTP.
- **Im Zweifel: Connect.**

### Русский (перевод выполнен с помощью ИИ и литературно отредактирован; носителями языка пока не проверен)
**Это не варианты, из которых нужно выбрать один, а технологии разных уровней.** Заменить REST могут только Connect, gRPC и GraphQL; WASM же — это способ создания пользовательского интерфейса.

| Термин | Что это | Заменяет REST? |
|---|---|---|
| REST | Стиль построения API, при котором операции выражаются через URL и HTTP-методы | отменяется |
| **Connect** | `POST /Сервис/Метод` с телом в формате JSON; вызывается как из `curl`, так и из браузерного `fetch` | да (**выбран**) |
| **gRPC** | Тот же принцип «сервис — метод», но поверх двоичного формата protobuf и HTTP/2; работает быстро и хорошо подходит для потоковой передачи | да (**для связи между сервисами**) |
| gRPC-web | Адаптированный для браузера вариант gRPC | да (при необходимости) |
| GraphQL | Язык запросов: клиент запрашивает ровно те поля, которые ему нужны | частично (**только запросы на чтение**) |
| WASM | Позволяет выполнять код на Rust прямо в браузере | нет (технология для интерфейса) |

- **API, которые вызываются из браузерных интерфейсов или через `curl`, строятся на Connect** (POST; JSON читается и записывается через RS-JSON). Браузерный WASM не умеет работать с «чистым» gRPC. С Connect достаточно обычного `fetch`, а `prost` не требуется — размер WASM-файла не увеличивается.
- **Для обмена между сервисами (сервер — сервер, ресурсоёмкие задачи, потоковая передача) применяется gRPC.** В большинстве случаев достаточно Connect; на gRPC переходят только там, где потоковая передача действительно необходима.
- **GraphQL используется только для чтения**, когда нужно получить сразу несколько связанных данных. Для записи (операций) он не применяется.
- **WASM по-прежнему остаётся технологией для интерфейса.** Изменилось лишь то, что вызываемый им API теперь построен на Connect.
- Интерфейсы, форма которых задана внешним стандартом (ACME, обратные вызовы OAuth, вебхуки сторонних сервисов, проверки работоспособности, статические файлы, OTLP), остаются обычным HTTP.
- **Если есть сомнения — выбирайте Connect.**
