#!/bin/sh
# RS-LinkFusion インストールスクリプト(AlmaLinux/Ubuntu/Debian/Fedora/RHEL等、
# systemdを使う主要Linuxディストリ共通)。
#
# **正直な開示**: このバイナリはUbuntu 20.04以降・Debian 11以降・
# AlmaLinux 8以降等、比較的新しいglibcを持つディストリ向けにビルド
# されている(muslによる完全なディストリ非依存の静的リンクは対象外、
# 詳細は .github/workflows/release.yml のコメント参照)。
#
# 使い方:
#   curl -fsSL https://github.com/aon-co-jp/RS-LinkFusion/releases/latest/download/rs-linkfusion-linux-x86_64.tar.gz | tar xz
#   sudo ./install.sh

set -eu

BIN_SRC="$(dirname "$0")/rs-linkfusion"
INSTALL_DIR="/usr/local/bin"
SERVICE_FILE="/etc/systemd/system/rs-linkfusion.service"

if [ "$(id -u)" -ne 0 ]; then
    echo "root権限で実行してください(例: sudo ./install.sh)" >&2
    exit 1
fi

if [ ! -f "$BIN_SRC" ]; then
    echo "rs-linkfusion バイナリが見つかりません($BIN_SRC)。同梱のtar.gzを展開したディレクトリで実行してください。" >&2
    exit 1
fi

# 電源プロファイル選択(2026-07-31追加、エコシステム共通方針):
# 省電力・省メモリ・常時電源接続(NPU/GPU自動対応)。省電力と常時電源接続
# はCPU方針が正反対のため排他、省メモリは独立した軸でどちらとも併用可能。
echo "==> 電源プロファイルを選択してください(番号を入力、Enterで既定の1):"
echo "    1) 省電力 (power-saving) [既定]"
echo "    2) 省メモリ (low-memory)"
echo "    3) 省電力+省メモリ (power-saving,low-memory)"
echo "    4) 常時電源接続 (always-on) — NPU/GPU自動対応"
echo "    5) 省メモリ+常時電源接続 (low-memory,always-on) — NPU/GPU自動対応"
read -r PROFILE_CHOICE
case "${PROFILE_CHOICE:-1}" in
    1) POWER_PROFILE="power-saving" ;;
    2) POWER_PROFILE="low-memory" ;;
    3) POWER_PROFILE="power-saving,low-memory" ;;
    4) POWER_PROFILE="always-on" ;;
    5) POWER_PROFILE="low-memory,always-on" ;;
    *) echo "不明な選択です。既定の power-saving を使用します。" >&2; POWER_PROFILE="power-saving" ;;
esac
echo "==> 選択された電源プロファイル: ${POWER_PROFILE}"

echo "==> バイナリを ${INSTALL_DIR}/rs-linkfusion へ配置"
install -m 755 "$BIN_SRC" "${INSTALL_DIR}/rs-linkfusion"

if [ ! -f "$SERVICE_FILE" ]; then
    echo "==> systemdサービスのひな形を作成(${SERVICE_FILE}、既定では無効のまま)"
    cat > "$SERVICE_FILE" << EOF
[Unit]
Description=RS-Link-Fusion - 複数WAN/LAN/WiFiボンディング通信トンネル
After=network.target

[Service]
Type=simple
Environment=RS_LINKFUSION_POWER_PROFILE=${POWER_PROFILE}
# serve側(実サービスがあるマシン)の例:
#   ExecStart=${INSTALL_DIR}/rs-linkfusion serve --bind 0.0.0.0:5900 --target 127.0.0.1:8080 --key <rs-linkfusion generate-keyで生成した鍵>
# connect側(ローカル)の例:
#   ExecStart=${INSTALL_DIR}/rs-linkfusion connect --listen 127.0.0.1:8080 --remote <serve側のホスト名> --remote-port 5900 --key <同じ鍵>
# 上記どちらかをコメント解除・編集してから `systemctl enable --now rs-linkfusion` すること。
ExecStart=${INSTALL_DIR}/rs-linkfusion generate-key
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload
else
    echo "==> 既存のsystemdサービスが見つかったため上書きしません(${SERVICE_FILE})。電源プロファイルを反映するには"
    echo "    sudo systemctl edit rs-linkfusion で Environment=RS_LINKFUSION_POWER_PROFILE=${POWER_PROFILE} を手動追記してください。"
fi

echo "==> 完了。まず鍵を生成し、次にserve/connectどちらの役割かに応じて設定してください:"
echo "    ${INSTALL_DIR}/rs-linkfusion generate-key"
echo "    sudo systemctl edit rs-linkfusion  # ExecStart を実際のserve/connectコマンドへ書き換え"
echo "    sudo systemctl enable --now rs-linkfusion"
