# PORTING.md — お引越し可能ファイル

他のプロジェクトへそのまま(または軽微な変更で)移植できる実装パターン
一覧。

## Android向け`network-interface`代替実装(`vendor/network-interface/src/target/linux.rs`、2026-08-05)

crates.io版`network-interface`(v2.0.5)の`NetworkInterface::show()`は
Android向け分岐を持つが、実際には`getifaddrs`/`freeifaddrs`(glibc/BSD
API)を呼んでおりBionic libcにリンクできず`undefined symbol`でリンク
失敗する。これは`aggligator-transport-tcp`のような、複数ネットワーク
インターフェースを列挙する既存クレートをAndroidへ移植する際に共通して
ぶつかる障壁であるため、対応パターンとして記録する。

回避策: `getifaddrs`を一切使わず、(1) `/proc/net/dev`からインター
フェース名を列挙、(2) `libc`crateが`ifreq`をlinux_like全般で公開して
いないため自前定義した`ifreq`構造体+生の`ioctl(SIOCGIFADDR/
SIOCGIFFLAGS)`でIPv4アドレス・ループバックフラグを取得する
`#[cfg(target_os = "android")]`専用実装に差し替える。`Cargo.toml`の
`[patch.crates-io]`でこのローカルフォークへ全体を向ければ、依存元
クレート(`aggligator-transport-tcp`等)のソース自体には手を入れずに
Android対応できる。

**移植時の正直な制約**: IPv6アドレス・MACアドレス・ブロードキャスト
アドレスは取得しない(IPv4のみ)。WiFi/USB-Ethernetボンディングのような
IPv4で足りる用途への適用に限定して判断すること。また、
`aggligator-transport-tcp`の接続確立(`bind_device`/`SO_BINDTODEVICE`)
はLinuxカーネル上`CAP_NET_RAW`権限を要求するため、この代替実装で
インターフェース列挙自体が通っても、非root Android実機では
`bind_device`側が`EPERM`で別途失敗する可能性が残る(未検証、
`rs-link-fusion/CLAUDE.md`のHANDOFF 2026-08-05参照)。

## `aggligator`によるユーザー空間マルチホーミング(`src/main.rs`)

`aggligator-transport-tcp::simple::{tcp_connect, tcp_server}`は、
`TcpConnector::set_multi_interface(true)`(デフォルト有効)により
ローカルの全ネットワークインターフェースを自動列挙し、各インター
フェース×各サーバーIPごとに個別TCPリンクを張って1つの論理接続へ
束ねる。カーネルMPTCP/SCTPが使えない環境(Windows等)でも同じ目的
(物理経路の冗長化・帯域合算)をユーザー空間で実現できる、実績のある
パターン(`open-web-server-wire::mptcp_channel`が原型)。

```rust
use aggligator_transport_tcp::simple as agg_tcp;

// サーバー側
agg_tcp::tcp_server(bind_addr, |stream| async move {
    // streamは AsyncRead + AsyncWrite を実装する束ねられた論理接続
}).await?;

// クライアント側
let stream = agg_tcp::tcp_connect(["server.example.com".to_string()], 5900).await?;
```

## 長さプレフィクス付き圧縮+暗号化フレームプロトコル(`src/framed.rs`)

`[len:u32 LE][圧縮+暗号化済みペイロード]`という単純な形式で、任意の
`AsyncRead`/`AsyncWrite`ストリーム上に任意サイズのメッセージを安全に
送受信する最小実装。トンネル・RPC・ミニVPN等、ストリーム型プロトコル
の上に「1メッセージ単位の圧縮+暗号化された往復」が必要などんな場面
にも移植できる。

```rust
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W, accel: &PayloadAccelerator, plaintext: &[u8],
) -> anyhow::Result<()> {
    let sealed = accel.seal(plaintext)?;
    let len = (sealed.len() as u32).to_le_bytes();
    writer.write_all(&len).await?;
    writer.write_all(&sealed).await?;
    writer.flush().await?;
    Ok(())
}
```

## 圧縮+暗号化ハードウェアアクセラレータ抽象化(`src/accel.rs`、移植元:
`open-web-server-wire::accel`、同パターンを自己完結で再実装)

`AccelBackend`列挙型で将来のハードウェア(GPU/NPU/専用アクセラレータ)
をAPI形状として先取りし、未実装のバックエンドが要求されてもpanicせず
`Cpu`へ安全にフォールバックしつつ`tracing::warn!`で可視化する設計。
呼び出し側のコードを変えずに将来ハードウェアが実装された時にそのまま
差し替わる。独立配布物(ダウンロードしてすぐ動く単体バイナリ)にする
場合は、この`open-web-server`への依存を持たせない自己完結実装のまま
コピーするのが移植コスト最小。

```rust
pub fn new(backend: AccelBackend, key: &[u8; 32]) -> Self {
    let effective = match backend {
        AccelBackend::Cpu => AccelBackend::Cpu,
        other => {
            tracing::warn!(requested = ?other, "accelerator backend not yet implemented, falling back to Cpu");
            AccelBackend::Cpu
        }
    };
    Self { backend: effective, cipher: ChaCha20Poly1305::new(Key::from_slice(key)) }
}
```

## ボンディング接続とローカルTCP接続の双方向リレー(`src/main.rs::relay`)

`tokio::io::split`で1本のストリーム(ボンディング接続)を読み書き
半分に分け、`tokio::try_join!`で双方向コピーを並行実行するパターン。
片方が`AsyncRead+AsyncWrite`を実装する任意のトンネル層、もう片方が
平文ローカルソケットという構図であれば、SSH的なポートフォワード・
リバースプロキシ全般にそのまま移植できる。

```rust
let (mut local_rd, mut local_wr) = local.into_split();
let (mut agg_rd, mut agg_wr) = tokio::io::split(agg_stream);

let to_agg = async { /* local_rd → framed::write_frame → agg_wr */ };
let to_local = async { /* framed::read_frame ← agg_rd → local_wr */ };
tokio::try_join!(to_agg, to_local)?;
```
### ハードウェアアクセラレータのフォールバックパターン（移植時の重要な教訓）

`accel.rs` の `PayloadAccelerator::new()` は、要求されたバックエンド（`AccelBackend::Gpu`）が利用できない場合に **自動的に CPU にフォールバック** する設計です。  
このパターンは、移植先の環境が GPU をサポートしていない場合でもアプリケーションが継続して動作することを保証します。

**移植時の注意**：
- GPU バックエンドを追加する場合、必ず `cfg(feature = "...")` で条件コンパイルし、フォールバック経路を用意してください。
- **訂正(2026-07-23、実機検証済み)**: 以前このファイルに「GT730はDirectX 12
  非対応」という誤った記述があったが、事実と異なる。**GT730はDirectX 12に
  対応している(Feature Level 11_0)**——`open-cuda`側`opencuda-directx`
  クレートで`D3D12CreateDevice`の実機成功・DXGIアダプタ列挙での
  `"NVIDIA GeForce GT 730"`取得・GPUディスパッチとCPU参照実装の完全一致を
  複数のテストで検証済み(詳細は`open-cuda`側`CLAUDE.md`のHANDOFF参照)。
  GT730が対応しないのはDirectX 12の新しいFeature Level(12_x系、Ray
  Tracing等)であり、「DirectX 12非対応」という表現自体が誤り。
  Vulkanバックエンドの追加は現時点で必須ではない(DirectX 12で
  GT730を含め動作確認済みのため)——非Windows環境(Linux/Android等)で
  GPU加速したい場合の選択肢として残る。
```

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
