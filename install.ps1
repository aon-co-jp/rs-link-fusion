# RS-LinkFusion インストールスクリプト(Windows / Windows Server 共通)。
#
# 使い方(管理者権限のPowerShellで):
#   Invoke-WebRequest -Uri "https://github.com/aon-co-jp/RS-LinkFusion/releases/latest/download/rs-linkfusion-windows-x86_64.zip" -OutFile rs-linkfusion.zip
#   Expand-Archive rs-linkfusion.zip -DestinationPath rs-linkfusion
#   cd rs-linkfusion
#   .\install.ps1

#Requires -RunAsAdministrator

$ErrorActionPreference = "Stop"

$InstallDir = "C:\Program Files\RS-LinkFusion"
$ServiceName = "RSLinkFusion"

Write-Host "==> インストール先: $InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

$BinSrc = Join-Path $PSScriptRoot "rs-linkfusion.exe"
if (-not (Test-Path $BinSrc)) {
    Write-Error "rs-linkfusion.exe が見つかりません($BinSrc)。zipを展開したディレクトリで実行してください。"
    exit 1
}
Copy-Item $BinSrc -Destination $InstallDir -Force

# 電源プロファイル選択(2026-07-31追加、エコシステム共通方針):
# 省電力・省メモリ・常時電源接続(NPU/GPU自動対応)。省電力と常時電源接続
# はCPU方針が正反対のため排他、省メモリは独立した軸でどちらとも併用可能。
Write-Host "==> 電源プロファイルを選択してください:"
Write-Host "    1) 省電力 (power-saving) [既定]"
Write-Host "    2) 省メモリ (low-memory)"
Write-Host "    3) 省電力+省メモリ (power-saving,low-memory)"
Write-Host "    4) 常時電源接続 (always-on) - NPU/GPU自動対応"
Write-Host "    5) 省メモリ+常時電源接続 (low-memory,always-on) - NPU/GPU自動対応"
$profileChoice = Read-Host "番号を入力 (Enterで既定の1)"
switch ($profileChoice) {
    "2" { $PowerProfile = "low-memory" }
    "3" { $PowerProfile = "power-saving,low-memory" }
    "4" { $PowerProfile = "always-on" }
    "5" { $PowerProfile = "low-memory,always-on" }
    default { $PowerProfile = "power-saving" }
}
Write-Host "==> 選択された電源プロファイル: $PowerProfile"
[Environment]::SetEnvironmentVariable("RS_LINKFUSION_POWER_PROFILE", $PowerProfile, "Machine")

# TUNゲートウェイ(gateway-serve/gateway-connect)に必要なwintun.dll。
# zip同梱時のみコピー(README.md参照、https://wintun.net/ から別途取得)。
$WintunSrc = Join-Path $PSScriptRoot "wintun.dll"
if (Test-Path $WintunSrc) {
    Copy-Item $WintunSrc -Destination $InstallDir -Force
    Write-Host "==> wintun.dll を配置しました(TUNゲートウェイ用)"
}

# GPUバックエンド(--gpu feature、chacha20カーネル)に必要なDXILシェーダー。
# zip同梱時のみコピー(未同梱でもCPUバックエンドへ安全にフォールバックする)。
$ShadersSrc = Join-Path $PSScriptRoot "shaders"
if (Test-Path $ShadersSrc) {
    Copy-Item $ShadersSrc -Destination $InstallDir -Recurse -Force
    Write-Host "==> shaders\ を配置しました(GPUバックエンド用、未配置時はCPUへ自動フォールバック)"
}

$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "==> 既存のWindowsサービスが見つかったため、バイナリのみ更新しました(再起動は行いません)"
    Write-Host "    手動で再起動する場合: Restart-Service $ServiceName"
} else {
    Write-Host "==> まず鍵を生成してください:"
    Write-Host "      & '$InstallDir\rs-linkfusion.exe' generate-key"
    Write-Host "==> Windowsサービスとして登録する場合の手順(serve側の例、connectの場合はサブコマンドを読み替え。"
    Write-Host "    --power-profile は上で選択した値〈$PowerProfile〉を明示指定することを推奨——サービスは環境変数を"
    Write-Host "    引き継がない場合があるため):"
    Write-Host "      New-Service -Name $ServiceName -BinaryPathName '$InstallDir\rs-linkfusion.exe --power-profile $PowerProfile serve --bind 0.0.0.0:5900 --target 127.0.0.1:8080 --key <上記の鍵>' -DisplayName 'RS-Link-Fusion' -StartupType Automatic"
    Write-Host "      Start-Service $ServiceName"
}

Write-Host "==> 完了。"
