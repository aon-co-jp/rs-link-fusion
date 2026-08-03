//! `rs-linkfusion` CLIエントリポイント。
//!
//! 複数WAN/LAN/WiFiインターフェース(`aggligator`が自動列挙する、OSが
//! 認識するすべてのネットワークインターフェース)を1つの論理接続へ
//! 束ね、その上に`framed`モジュールの圧縮+暗号化フレームで任意のTCP
//! トラフィックを流す「ミニVPN的トンネル」を提供する。
//!
//! - `serve`: リモート側。ボンディング接続を受け付け、ローカルの
//!   実サービス(例: `127.0.0.1:8080`)へリバースプロキシする。
//! - `connect`: ローカル側。ローカルポートで待ち受け、接続ごとに
//!   `serve`側へのボンディング接続を新規に張って転送する。
//! - `gateway-serve`/`gateway-connect`: TUN仮想アダプタ方式のフルVPN
//!   ゲートウェイ(`tun_gateway`モジュール参照)。固定1アドレスへの
//!   ポート転送ではなく、PC上のあらゆる通信をボンディング接続経由で
//!   流したい場合に使う。

mod accel;
mod framed;
#[cfg(feature = "gui")]
mod gui;
mod landing;
mod qos;
mod quality;
mod speedtest;
#[cfg(feature = "tun-gateway")]
mod tun_gateway;

use accel::{AccelBackend, PayloadAccelerator};
use aggligator_transport_tcp::simple as agg_tcp;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use quality::QualityTracker;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn default_history_path() -> PathBuf {
    PathBuf::from("speedtest-history.jsonl")
}

/// `qos_config`引数から`Qos`を組み立てる。`None`なら`None`(帯域制御
/// オフ)、`"default"`なら内蔵プリセット、それ以外はTOMLファイルパス
/// として読み込む。
fn load_qos(qos_config: Option<&str>) -> Result<Option<Arc<qos::Qos>>> {
    match qos_config {
        None => Ok(None),
        Some("default") => {
            tracing::info!("QoS: 内蔵プリセット(主要な動画/音楽配信サービスを10Mbpsへ制限)を使用");
            Ok(Some(Arc::new(qos::Qos::new(qos::QosConfig::default_streaming_preset()))))
        }
        Some(path) => {
            let config = qos::QosConfig::load(std::path::Path::new(path)).context("QoS設定ファイルの読み込みに失敗しました")?;
            tracing::info!(streaming_rate_mbps = config.streaming_rate_mbps, suffix_count = config.streaming_suffixes.len(), "QoS設定を読み込みました");
            Ok(Some(Arc::new(qos::Qos::new(config))))
        }
    }
}

/// `--accel`引数文字列を`AccelBackend`へ変換する。
fn parse_accel_backend(s: &str) -> Result<AccelBackend> {
    match s.to_lowercase().as_str() {
        "cpu" => Ok(AccelBackend::Cpu),
        "gpu" => Ok(AccelBackend::Gpu),
        other => anyhow::bail!("不明な --accel 値: {other}(cpu または gpu を指定してください)"),
    }
}

/// インストーラーの電源プロファイル選択(`open-raid-z`で定めたエコ
/// システム共通方針、2026-07-31追加)。**排他なのは「省電力」対
/// 「常時電源接続」の組のみ**(ユーザー指示を時系列で整理した結果:
/// 「省電力+省メモリは選択可能」→「省メモリ+常時電源接続も選択可能」
/// →つまり「省メモリ」はCPU/電源方針とは独立した軸〈メモリ消費量〉
/// であり、省電力・常時電源接続いずれとも併用できる。一方「省電力」と
/// 「常時電源接続」はCPU使用率について正反対の方針〈抑える/抑えない〉
/// のため、この2つのみ同時選択不可とする)。CLIではカンマ区切りで
/// 指定する(例: `--power-profile power-saving,low-memory`、
/// `--power-profile low-memory,always-on`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PowerProfile {
    /// CPU使用率・ポーリング間隔を抑える。
    power_saving: bool,
    /// メモリ確保量・キャッシュサイズを抑える。
    low_memory: bool,
    /// フル性能・ハードウェアアクセラレータ(NPU/GPU)自動有効化。
    /// `power_saving`/`low_memory`のいずれかと同時にはできない。
    always_on: bool,
}

impl PowerProfile {
    fn parse(s: &str) -> Result<Self> {
        let mut power_saving = false;
        let mut low_memory = false;
        let mut always_on = false;
        for part in s.split(',').map(|p| p.trim().to_lowercase().replace('_', "-")) {
            match part.as_str() {
                "power-saving" => power_saving = true,
                "low-memory" => low_memory = true,
                "always-on" => always_on = true,
                other => anyhow::bail!("不明な --power-profile 値: {other}(power-saving / low-memory / always-on のいずれか、またはpower-saving,low-memoryのようにカンマ区切りで複数指定してください)"),
            }
        }
        if always_on && power_saving {
            anyhow::bail!("--power-profile: always-onとpower-saving(CPU使用率について正反対の方針)は同時に指定できません(排他)。low-memoryとはそれぞれ併用可能です");
        }
        if !power_saving && !low_memory && !always_on {
            anyhow::bail!("--power-profile: 少なくとも1つの値を指定してください");
        }
        Ok(Self { power_saving, low_memory, always_on })
    }

    /// tokioランタイムをシングルスレッドで動かすべきか。
    /// **`always_on`のみで判定する**(`always_on`かつ`low_memory`の
    /// 併用時に「常時電源接続=フル性能」と「シングルスレッド」が矛盾
    /// しないようにするため——CPU並列度は`always_on`の有無で決め、
    /// `low_memory`はメモリ確保量という別軸として扱う。**正直な開示**:
    /// `low_memory`単体でのメモリ確保量削減自体〈バッファ/キャッシュ
    /// サイズの調整〉はまだ実装しておらず、現状は電源プロファイルの
    /// ログ出力とスレッド数決定への関与のみが実効的な差分。
    fn wants_single_thread_runtime(&self) -> bool {
        !self.always_on
    }
}

const CHUNK_SIZE: usize = 16 * 1024;

#[derive(Parser)]
#[command(
    name = "rs-linkfusion",
    version,
    about = "複数WAN/LAN/WiFiインターフェースをaggligatorで束ね(ボンディング)、通信の高速化・安定化を実現するトンネル"
)]
struct Cli {
    /// 電源プロファイル(インストーラーで選択した値をsystemd
    /// `Environment=`経由で渡す想定、2026-07-31追加)。
    #[arg(long, env = "RS_LINKFUSION_POWER_PROFILE", default_value = "power-saving")]
    power_profile: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 新しい暗号鍵(32バイト、hex表示)を生成する。serve/connect双方に同じ鍵を渡すこと。
    GenerateKey,
    /// リモート側: ボンディング接続を受け付け、ローカルサービスへリバースプロキシする。
    ///
    /// 各引数は環境変数でも指定できる(`RS_LINKFUSION_*`)。systemd等の
    /// サービスマネージャからは`ExecStart`に引数を書かず、
    /// `Environment=`だけで設定できるようにするため。
    Serve {
        /// ボンディング接続の受け付けアドレス(例: 0.0.0.0:5900)
        #[arg(long, env = "RS_LINKFUSION_BIND")]
        bind: SocketAddr,
        /// 転送先のローカルサービスアドレス(例: 127.0.0.1:8080)
        #[arg(long, env = "RS_LINKFUSION_TARGET")]
        target: SocketAddr,
        /// `generate-key`で生成したhex鍵
        #[arg(long, env = "RS_LINKFUSION_KEY")]
        key: String,
        /// 圧縮+暗号化のアクセラレータバックエンド(cpu/gpu)。gpuは`gpu`
        /// feature必須、GPU初期化に失敗した場合は安全にcpuへフォール
        /// バックする。
        #[arg(long, env = "RS_LINKFUSION_ACCEL", default_value = "cpu")]
        accel: String,
    },
    /// ローカル側: ローカルポートで待ち受け、ボンディング接続へ転送する。
    ///
    /// 各引数は環境変数でも指定できる(`RS_LINKFUSION_*`)。
    Connect {
        /// ローカル待ち受けアドレス(例: 127.0.0.1:8080)
        #[arg(long, env = "RS_LINKFUSION_LISTEN")]
        listen: SocketAddr,
        /// 接続先ホスト名/IPアドレス(カンマ区切りで複数指定可、`serve`側の`--bind`のIP/ホスト名)
        #[arg(long, env = "RS_LINKFUSION_REMOTE", value_delimiter = ',')]
        remote: Vec<String>,
        /// 接続先ポート(`serve`側の`--bind`のポート)
        #[arg(long, env = "RS_LINKFUSION_REMOTE_PORT")]
        remote_port: u16,
        /// `generate-key`で生成したhex鍵(`serve`側と同じ値)
        #[arg(long, env = "RS_LINKFUSION_KEY")]
        key: String,
        /// 圧縮+暗号化のアクセラレータバックエンド(cpu/gpu)。`serve`側と
        /// 同じ値を指定する必要はない(通信路の暗号化とは独立、双方が
        /// それぞれ自分のフレームを自分の設定で処理する)。
        #[arg(long, env = "RS_LINKFUSION_ACCEL", default_value = "cpu")]
        accel: String,
    },
    /// TUNゲートウェイ・リモート側(典型的にはLinux VPS)。IPフォワーディング/
    /// NAT(MASQUERADE)の有効化は自動で行わない(README.md参照、手動設定が必要)。
    /// `tun-gateway`feature必須(Androidクロスビルドでは`tun-rs`が
    /// `DeviceBuilder`をサポートしないため既定で無効化される、2026-08-03追記)。
    #[cfg(feature = "tun-gateway")]
    GatewayServe {
        /// ボンディング接続の受け付けアドレス(例: 0.0.0.0:5900)
        #[arg(long, env = "RS_LINKFUSION_BIND")]
        bind: SocketAddr,
        /// TUNインターフェースに割り当てるIPv4アドレス
        #[arg(long, env = "RS_LINKFUSION_TUN_ADDR", default_value = "10.66.0.1")]
        tun_addr: Ipv4Addr,
        /// TUNインターフェースのプレフィクス長
        #[arg(long, env = "RS_LINKFUSION_TUN_PREFIX", default_value_t = 24)]
        tun_prefix: u8,
        /// TUNインターフェースのMTU
        #[arg(long, env = "RS_LINKFUSION_MTU", default_value_t = 1400)]
        mtu: u16,
        /// `generate-key`で生成したhex鍵
        #[arg(long, env = "RS_LINKFUSION_KEY")]
        key: String,
        /// QoS設定TOMLファイル(streaming_suffixes/streaming_rate_mbps)。
        /// 未指定なら帯域制御は行わない(既定オフ)。`default`を渡すと
        /// 主要な動画/音楽配信サービスの内蔵プリセットを使う。
        #[arg(long, env = "RS_LINKFUSION_QOS_CONFIG")]
        qos_config: Option<String>,
        /// 圧縮+暗号化のアクセラレータバックエンド(cpu/gpu)。QoSで
        /// 帯域制限される「高音質」トラフィックと、無制限の「高速」
        /// トラフィックの両方に同じバックエンドが使われる(GPU暗号化の
        /// 恩恵はどちらの層にも及ぶ、ユーザー選択制)。
        #[arg(long, env = "RS_LINKFUSION_ACCEL", default_value = "cpu")]
        accel: String,
    },
    /// TUNゲートウェイ・ローカル側(Windows等)。管理者権限、Windowsでは
    /// `wintun.dll`が実行ファイルと同じディレクトリに必要(README.md参照)。
    /// デフォルトルートのTUN経由への切り替えは自動で行わない。
    /// `tun-gateway`feature必須(2026-08-03追記、上記`GatewayServe`参照)。
    #[cfg(feature = "tun-gateway")]
    GatewayConnect {
        /// 接続先ホスト名/IPアドレス(カンマ区切りで複数指定可)
        #[arg(long, env = "RS_LINKFUSION_REMOTE", value_delimiter = ',')]
        remote: Vec<String>,
        /// 接続先ポート(`gateway-serve`側の`--bind`のポート)
        #[arg(long, env = "RS_LINKFUSION_REMOTE_PORT")]
        remote_port: u16,
        /// TUNインターフェースに割り当てるIPv4アドレス
        #[arg(long, env = "RS_LINKFUSION_TUN_ADDR", default_value = "10.66.0.2")]
        tun_addr: Ipv4Addr,
        /// TUNインターフェースのプレフィクス長
        #[arg(long, env = "RS_LINKFUSION_TUN_PREFIX", default_value_t = 24)]
        tun_prefix: u8,
        /// TUNインターフェースのMTU(`gateway-serve`側と一致させること)
        #[arg(long, env = "RS_LINKFUSION_MTU", default_value_t = 1400)]
        mtu: u16,
        /// `generate-key`で生成したhex鍵(`gateway-serve`側と同じ値)
        #[arg(long, env = "RS_LINKFUSION_KEY")]
        key: String,
        /// QoS設定TOMLファイル。未指定なら帯域制御は行わない(既定オフ)。
        /// `default`で主要な動画/音楽配信サービスの内蔵プリセットを使う。
        #[arg(long, env = "RS_LINKFUSION_QOS_CONFIG")]
        qos_config: Option<String>,
        /// 圧縮+暗号化のアクセラレータバックエンド(cpu/gpu)。
        #[arg(long, env = "RS_LINKFUSION_ACCEL", default_value = "cpu")]
        accel: String,
    },
    /// ネット速度測定(M-Lab/ndt7)・自動記録・履歴管理。
    SpeedTest {
        #[command(subcommand)]
        command: SpeedTestCommand,
    },
    /// GUIウィンドウを起動する(「速度測定」ボタン等)。`gui` feature必須。
    Gui,
    /// ダウンロード案内・電源プロファイル説明ページ(`static/landing.html`)
    /// を配信する最小限のHTTPサーバー(2026-07-31追加)。`open-web-server`
    /// 配下にパスプレフィックス付きでマウントされる想定
    /// (`open-redmine`の`web/src/lib.rs`の`BASE_PATH`と同種の罠を踏まない
    /// よう、このサーバー自体はプレフィックスを意識せず常に同じHTMLを
    /// 返す静的配信のみ——リンク先はすべて絶対URL/ドメイン相対で埋め込み
    /// 済み)。
    Landing {
        /// 待ち受けアドレス(例: 0.0.0.0:8600)
        #[arg(long, env = "RS_LINKFUSION_LANDING_BIND", default_value = "127.0.0.1:8600")]
        bind: SocketAddr,
    },
}

#[derive(Subcommand)]
enum SpeedTestCommand {
    /// M-Lab(ndt7)で1回速度測定する
    Run {
        /// 記録に付けるラベル(例: baseline, accelerated)
        #[arg(long, default_value = "manual")]
        label: String,
        /// 対話的な同意確認をスキップする
        #[arg(long)]
        yes: bool,
        /// 履歴ファイルのパス
        #[arg(long, default_value = "speedtest-history.jsonl")]
        history: PathBuf,
    },
    /// M-Lab(ndt7)を一定間隔で自動測定・自動記録し続ける(確認なし、Ctrl+Cで終了)
    Watch {
        /// 測定間隔(分)
        #[arg(long, default_value_t = 60)]
        interval_minutes: u64,
        #[arg(long, default_value = "auto")]
        label: String,
        #[arg(long, default_value = "speedtest-history.jsonl")]
        history: PathBuf,
    },
    /// gate02/osakagas等、非公式サイトを手動で開いて読み取った値を記録する
    RecordManual {
        /// 測定元(例: gate02, osakagas)
        #[arg(long)]
        source: String,
        #[arg(long, default_value = "manual")]
        label: String,
        #[arg(long)]
        download_mbps: f64,
        #[arg(long)]
        upload_mbps: f64,
        #[arg(long, default_value = "speedtest-history.jsonl")]
        history: PathBuf,
    },
    /// gate02/osakagas等、手動確認用サイトのURL一覧を表示する
    Links,
    /// 記録済みの測定履歴を表示する
    History {
        #[arg(long, default_value = "speedtest-history.jsonl")]
        history: PathBuf,
    },
    /// 古くなった記録を確認のうえまとめて削除する
    Prune {
        /// これより古い記録を削除対象にする(日数)
        #[arg(long, default_value_t = 90)]
        older_than_days: i64,
        /// 確認をスキップする
        #[arg(long)]
        yes: bool,
        #[arg(long, default_value = "speedtest-history.jsonl")]
        history: PathBuf,
    },
}

/// `--accel`の実効値を決定する。電源プロファイルが`always-on`かつ
/// ユーザーが明示的に`gpu`を指定していない(既定値`cpu`のまま)場合のみ
/// `gpu`へ自動アップグレードする——ユーザーが明示的に`cpu`を指定した
/// 場合はそちらを尊重する、という判断(電源プロファイルは「既定値の
/// 賢い選択」であって、明示指定を上書きするものではない)。
fn effective_accel<'a>(power_profile: PowerProfile, requested_accel: &'a str) -> &'a str {
    if power_profile.always_on && requested_accel == "cpu" {
        tracing::info!("power-profile=always-on: --accelが既定値(cpu)のため、ハードウェアアクセラレータ(gpu)へ自動アップグレードします");
        "gpu"
    } else {
        requested_accel
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let power_profile = PowerProfile::parse(&cli.power_profile)?;

    // 電源プロファイルに応じてtokioランタイムの構成を変える
    // (2026-07-31追加、インストーラーの電源プロファイル選択機能):
    // power-saving/low-memoryはシングルスレッド(current_thread)で
    // CPU使用率・メモリ確保量を抑え、always-onは全論理コアを使う
    // multi_threadでフル性能を出す。
    let runtime = if power_profile.wants_single_thread_runtime() {
        tracing::info!(profile = ?power_profile, "電源プロファイル: シングルスレッドランタイムで起動します");
        tokio::runtime::Builder::new_current_thread().enable_all().build()?
    } else {
        tracing::info!(profile = ?power_profile, "電源プロファイル: マルチスレッド(全論理コア)ランタイムで起動します");
        tokio::runtime::Builder::new_multi_thread().enable_all().build()?
    };

    runtime.block_on(async_main(cli.command, power_profile))
}

async fn async_main(command: Command, power_profile: PowerProfile) -> Result<()> {
    match command {
        Command::GenerateKey => {
            let key = PayloadAccelerator::generate_key();
            println!("{}", encode_hex(&key));
        }
        Command::Serve { bind, target, key, accel } => {
            let key = decode_hex_key(&key)?;
            let accel = parse_accel_backend(effective_accel(power_profile, &accel))?;
            run_serve(bind, target, key, accel).await?;
        }
        Command::Connect { listen, remote, remote_port, key, accel } => {
            let key = decode_hex_key(&key)?;
            let accel = parse_accel_backend(effective_accel(power_profile, &accel))?;
            run_connect(listen, remote, remote_port, key, accel).await?;
        }
        #[cfg(feature = "tun-gateway")]
        Command::GatewayServe { bind, tun_addr, tun_prefix, mtu, key, qos_config, accel } => {
            let key = decode_hex_key(&key)?;
            let qos = load_qos(qos_config.as_deref())?;
            let accel = parse_accel_backend(effective_accel(power_profile, &accel))?;
            run_gateway_serve(bind, tun_addr, tun_prefix, mtu, key, qos, accel).await?;
        }
        #[cfg(feature = "tun-gateway")]
        Command::GatewayConnect { remote, remote_port, tun_addr, tun_prefix, mtu, key, qos_config, accel } => {
            let key = decode_hex_key(&key)?;
            let qos = load_qos(qos_config.as_deref())?;
            let accel = parse_accel_backend(effective_accel(power_profile, &accel))?;
            run_gateway_connect(remote, remote_port, tun_addr, tun_prefix, mtu, key, qos, accel).await?;
        }
        Command::SpeedTest { command } => run_speedtest_command(command).await?,
        Command::Landing { bind } => landing::run(bind).await?,
        Command::Gui => {
            #[cfg(feature = "gui")]
            {
                gui::run(default_history_path())?;
            }
            #[cfg(not(feature = "gui"))]
            {
                anyhow::bail!("この実行ファイルは`gui` featureを無効にしてビルドされているため、GUIは使えません");
            }
        }
    }

    Ok(())
}

async fn run_speedtest_command(command: SpeedTestCommand) -> Result<()> {
    match command {
        SpeedTestCommand::Run { label, yes, history } => {
            let record = speedtest::run(label, &history, yes).await?;
            println!(
                "download: {:.1} Mbps / upload: {:.1} Mbps / min RTT: {}",
                record.download_mbps,
                record.upload_mbps,
                record.min_rtt_ms.map(|v| format!("{v:.1} ms")).unwrap_or_else(|| "N/A".to_string())
            );
        }
        SpeedTestCommand::Watch { interval_minutes, label, history } => {
            speedtest::watch(label, &history, std::time::Duration::from_secs(interval_minutes * 60), false).await?;
        }
        SpeedTestCommand::RecordManual { source, label, download_mbps, upload_mbps, history } => {
            speedtest::record_manual(source, label, download_mbps, upload_mbps, &history)?;
            println!("記録しました。");
        }
        SpeedTestCommand::Links => {
            println!("M-Lab(自動測定対応): `rs-linkfusion speedtest run` で実行できます。");
            for (name, url) in speedtest::MANUAL_REFERENCE_SITES {
                println!("{name}(手動確認用): {url}");
            }
        }
        SpeedTestCommand::History { history } => {
            for record in speedtest::load_history(&history)? {
                println!(
                    "[{}] {} / {}: down {:.1} Mbps, up {:.1} Mbps (interfaces: {})",
                    record.recorded_at, record.source, record.label, record.download_mbps, record.upload_mbps, record.environment.interface_count
                );
            }
        }
        SpeedTestCommand::Prune { older_than_days, yes, history } => {
            speedtest::prune_older_than(&history, older_than_days, yes)?;
        }
    }
    Ok(())
}

async fn run_serve(bind: SocketAddr, target: SocketAddr, key: [u8; 32], accel_backend: AccelBackend) -> Result<()> {
    let accel = Arc::new(PayloadAccelerator::new(accel_backend, &key));
    tracing::info!(%bind, %target, backend = ?accel.backend(), "starting bonded tunnel server");

    agg_tcp::tcp_server(bind, move |stream| {
        let accel = Arc::clone(&accel);
        async move {
            if let Err(e) = handle_serve_connection(stream, target, accel).await {
                tracing::warn!(error = %e, "serve connection ended with error");
            }
        }
    })
    .await
    .context("bonded tcp_server failed")?;

    Ok(())
}

async fn handle_serve_connection(
    agg_stream: aggligator::alc::Stream,
    target: SocketAddr,
    accel: Arc<PayloadAccelerator>,
) -> Result<()> {
    let local = TcpStream::connect(target).await.context("connecting to local target service")?;
    relay(agg_stream, local, accel).await
}

async fn run_connect(
    listen: SocketAddr, remote_hosts: Vec<String>, remote_port: u16, key: [u8; 32], accel_backend: AccelBackend,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(listen).await.context("binding local listen address")?;
    let accel = Arc::new(PayloadAccelerator::new(accel_backend, &key));
    tracing::info!(%listen, ?remote_hosts, remote_port, backend = ?accel.backend(), "starting bonded tunnel client");

    loop {
        let (local, peer) = listener.accept().await?;
        let accel = Arc::clone(&accel);
        let hosts = remote_hosts.clone();

        tokio::spawn(async move {
            match agg_tcp::tcp_connect(hosts, remote_port).await {
                Ok(agg_stream) => {
                    if let Err(e) = relay(agg_stream, local, accel).await {
                        tracing::warn!(error = %e, %peer, "connect relay ended with error");
                    }
                }
                Err(e) => tracing::warn!(error = %e, %peer, "failed to establish bonded connection"),
            }
        });
    }
}

/// TUNゲートウェイ・リモート側。1接続のみを受け付け、TUNデバイスと
/// ボンディング接続の間でIPパケットを中継する(複数クライアント同時
/// 接続時のパケット混線防止は未実装、単一クライアント前提の設計)。
#[cfg(feature = "tun-gateway")]
async fn run_gateway_serve(
    bind: SocketAddr,
    tun_addr: Ipv4Addr,
    tun_prefix: u8,
    mtu: u16,
    key: [u8; 32],
    qos: Option<Arc<qos::Qos>>,
    accel_backend: AccelBackend,
) -> Result<()> {
    let accel = Arc::new(PayloadAccelerator::new(accel_backend, &key));
    let tun = tun_gateway::create_tun_device(tun_addr, tun_prefix, mtu)?;
    tracing::info!(%bind, %tun_addr, tun_prefix, mtu, backend = ?accel.backend(), qos_enabled = qos.is_some(), "starting TUN gateway server");

    agg_tcp::tcp_server(bind, move |stream| {
        let accel = Arc::clone(&accel);
        let tun = Arc::clone(&tun);
        let quality = Arc::new(QualityTracker::new());
        let qos = qos.clone();
        async move {
            if let Err(e) = tun_gateway::relay_packets(tun, stream, accel, mtu as usize, quality, qos).await {
                tracing::warn!(error = %e, "gateway-serve relay ended with error");
            }
        }
    })
    .await
    .context("bonded tcp_server failed")?;

    Ok(())
}

/// TUNゲートウェイ・ローカル側。TUNデバイスを作成し、`remote`へ
/// ボンディング接続を張ってIPパケットを中継する。
///
/// **自動再接続(ユーザー指示、2026-07-23)**: WAN/LAN/WiFiの構成が
/// システム側で変化しても(回線切断・新規接続・全リンク一時喪失等)、
/// このループが無人で再接続を試み続ける。個々の物理インターフェース
/// の追加/削除自体は`aggligator`(`TcpConnector::link_tags`)が内部で
/// 10秒間隔(デフォルト)で自動再走査しているため、リンクが1本でも
/// 生きていればこのループの外側で自動的に吸収される——ここで扱うのは
/// 「ボンディング接続が完全に切断された(全リンク喪失)場合の、
/// 接続そのものの再確立」。再試行間隔はRS-SmartTCPの
/// `AdaptivePolicy`(実測RTT/ジッターに基づくFast/Slow判定)に従って
/// 自動調整される(光回線級なら短い間隔、通常回線なら保守的な間隔)。
#[cfg(feature = "tun-gateway")]
async fn run_gateway_connect(
    remote: Vec<String>,
    remote_port: u16,
    tun_addr: Ipv4Addr,
    tun_prefix: u8,
    mtu: u16,
    key: [u8; 32],
    qos: Option<Arc<qos::Qos>>,
    accel_backend: AccelBackend,
) -> Result<()> {
    let accel = Arc::new(PayloadAccelerator::new(accel_backend, &key));
    let tun = tun_gateway::create_tun_device(tun_addr, tun_prefix, mtu)?;
    let quality = Arc::new(QualityTracker::new());
    tracing::info!(?remote, remote_port, %tun_addr, tun_prefix, mtu, backend = ?accel.backend(), qos_enabled = qos.is_some(), "starting TUN gateway client (auto-reconnect enabled)");

    loop {
        tracing::info!(?remote, remote_port, "establishing bonded connection to gateway-serve");
        match agg_tcp::tcp_connect(remote.clone(), remote_port).await {
            Ok(agg_stream) => {
                tracing::info!("bonded connection established, relaying packets");
                if let Err(e) = tun_gateway::relay_packets(
                    Arc::clone(&tun),
                    agg_stream,
                    Arc::clone(&accel),
                    mtu as usize,
                    Arc::clone(&quality),
                    qos.clone(),
                )
                .await
                {
                    tracing::warn!(error = %e, "gateway-connect relay ended, will attempt to reconnect");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to establish bonded connection, will retry");
            }
        }

        quality.log_status();
        let backoff = quality.policy().retry_backoff();
        tracing::info!(?backoff, mode = ?quality.policy().mode(), "waiting before reconnect attempt (RS-SmartTCP adaptive backoff)");
        tokio::time::sleep(backoff).await;
    }
}

/// ボンディング接続(圧縮+暗号化フレーム)とローカルTCP接続(平文)の間で
/// 双方向にデータを中継する。
async fn relay(agg_stream: aggligator::alc::Stream, local: TcpStream, accel: Arc<PayloadAccelerator>) -> Result<()> {
    let (mut local_rd, mut local_wr) = local.into_split();
    let (mut agg_rd, mut agg_wr) = tokio::io::split(agg_stream);
    let quality = QualityTracker::new();

    let to_agg = async {
        let mut buf = vec![0u8; CHUNK_SIZE];
        loop {
            let n = local_rd.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            framed::write_frame(&mut agg_wr, &accel, &buf[..n]).await?;
        }
        anyhow::Ok(())
    };

    let to_local = async {
        loop {
            let started = Instant::now();
            match framed::read_frame(&mut agg_rd, &accel).await? {
                Some(data) => {
                    quality.record_round_trip(started);
                    local_wr.write_all(&data).await?;
                }
                None => break,
            }
        }
        anyhow::Ok(())
    };

    let result = tokio::try_join!(to_agg, to_local);
    quality.log_status();
    result?;
    Ok(())
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex_key(s: &str) -> Result<[u8; 32]> {
    if s.len() != 64 {
        anyhow::bail!("key must be 64 hex characters (32 bytes), got {} characters", s.len());
    }
    let mut out = [0u8; 32];
    for (i, chunk) in out.iter_mut().enumerate() {
        let byte_str = &s[i * 2..i * 2 + 2];
        *chunk = u8::from_str_radix(byte_str, 16).context("key must be valid hex")?;
    }
    Ok(out)
}

#[cfg(test)]
mod power_profile_tests {
    use super::*;

    #[test]
    fn power_saving_and_low_memory_can_combine() {
        let p = PowerProfile::parse("power-saving,low-memory").unwrap();
        assert!(p.power_saving && p.low_memory && !p.always_on);
        assert!(p.wants_single_thread_runtime());
    }

    #[test]
    fn low_memory_and_always_on_can_combine() {
        let p = PowerProfile::parse("low-memory,always-on").unwrap();
        assert!(p.low_memory && p.always_on && !p.power_saving);
        // always_onが優先されマルチスレッドになる(矛盾を避ける設計)。
        assert!(!p.wants_single_thread_runtime());
    }

    #[test]
    fn power_saving_and_always_on_are_mutually_exclusive() {
        assert!(PowerProfile::parse("power-saving,always-on").is_err());
        assert!(PowerProfile::parse("always-on,power-saving").is_err());
    }

    #[test]
    fn always_on_alone_wants_multi_thread_and_gpu_upgrade() {
        let p = PowerProfile::parse("always-on").unwrap();
        assert!(!p.wants_single_thread_runtime());
        assert_eq!(effective_accel(p, "cpu"), "gpu");
        // 明示的にcpuを指定したわけではなく既定値のcpuのみアップグレード対象。
    }

    #[test]
    fn power_saving_alone_does_not_upgrade_accel() {
        let p = PowerProfile::parse("power-saving").unwrap();
        assert!(p.wants_single_thread_runtime());
        assert_eq!(effective_accel(p, "cpu"), "cpu");
    }

    #[test]
    fn empty_value_is_rejected() {
        assert!(PowerProfile::parse("").is_err());
    }

    #[test]
    fn unknown_value_is_rejected() {
        assert!(PowerProfile::parse("turbo-mode").is_err());
    }
}
