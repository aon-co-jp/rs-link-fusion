package tokyo.runo.rslinkfusion

import android.os.Bundle
import android.widget.Button
import android.widget.EditText
import android.widget.RadioGroup
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import java.io.BufferedReader
import java.io.File
import java.io.InputStreamReader
import java.net.InetSocketAddress
import java.net.Socket
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * RS-Link-Fusion Android版シェル(2026-08-05新規実装)。
 *
 * **スコープ(CLAUDE.md 2026-08-05 HANDOFF参照)**: TUN仮想アダプタによる
 * フルVPN(`gateway-serve`/`gateway-connect`、`tun_gateway.rs`)は
 * AndroidのVpnServiceモデルに未対応のため今回のスコープ外。このアプリが
 * 起動するのは`serve`/`connect`(単純なTCPポート転送・ボンディング)の
 * みで、同梱ネイティブバイナリは`--no-default-features`
 * (`tun-gateway`feature無し)でクロスビルドしたもの。
 *
 * 参照実装: `open-easy-web/android`・`open-web-server/android`と同じ
 * 「単一Activity+ProcessBuilderでネイティブバイナリ起動」設計。
 * ただし`rs-linkfusion`自体はHTTPサーバーではないため`/healthz`相当の
 * ものが無く、代わりに以下で起動確認する:
 *   - `connect`モード: ローカルリスンアドレスへ実際にTCP接続してみて、
 *     接続が受理されること(=ローカルの`accept`ループが動いていること)
 *     を確認する。
 *   - `serve`モード: ネイティブプロセスが一定時間後も生存していること
 *     (`Process.isAlive`)のみで確認する(`serve`はボンディング接続が
 *     来るまで能動的な確認手段が無いため)。
 *
 * **正直な開示**: この開発環境には実機のUSB-Ethernetアダプタが無いため、
 * WiFi+USB-Ethernet同時ボンディングの実機E2E検証はできていない。
 * ビルド成功・単体ロジック・可能な範囲でのエミュレータ起動確認までが
 * スコープ(詳細はCLAUDE.md参照)。
 */
class MainActivity : AppCompatActivity() {

    private var serverProcess: Process? = null
    private lateinit var networkBinder: NetworkBinder

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        val networkStatusText = findViewById<TextView>(R.id.networkStatusText)
        val modeRadioGroup = findViewById<RadioGroup>(R.id.modeRadioGroup)
        val keyInput = findViewById<EditText>(R.id.keyInput)
        val localAddrInput = findViewById<EditText>(R.id.localAddrInput)
        val remoteHostInput = findViewById<EditText>(R.id.remoteHostInput)
        val remotePortInput = findViewById<EditText>(R.id.remotePortInput)
        val generateKeyButton = findViewById<Button>(R.id.generateKeyButton)
        val startButton = findViewById<Button>(R.id.startButton)
        val stopButton = findViewById<Button>(R.id.stopButton)
        val statusText = findViewById<TextView>(R.id.statusText)
        val logText = findViewById<TextView>(R.id.logText)

        networkBinder = NetworkBinder(this)
        networkBinder.start { status ->
            runOnUiThread {
                val wifi = if (status.wifiAvailable) {
                    "WiFi: ✅ (${status.wifiInterfaceName ?: "?"})"
                } else {
                    "WiFi: ❌"
                }
                val eth = if (status.ethernetAvailable) {
                    "Ethernet(USB-LAN): ✅ (${status.ethernetInterfaceName ?: "?"})"
                } else {
                    "Ethernet(USB-LAN): ❌ (未接続、またはこの端末はUSB-Ethernet未対応)"
                }
                networkStatusText.text = "$wifi / $eth"
            }
        }

        generateKeyButton.setOnClickListener {
            generateKeyButton.isEnabled = false
            CoroutineScope(Dispatchers.Main).launch {
                val log = StringBuilder()
                val key = withContext(Dispatchers.IO) { runOneShot(listOf("generate-key"), log) }
                if (key != null) {
                    keyInput.setText(key)
                }
                logText.text = log.toString()
                generateKeyButton.isEnabled = true
            }
        }

        startButton.setOnClickListener {
            startButton.isEnabled = false
            val isConnect = modeRadioGroup.checkedRadioButtonId == R.id.modeConnect
            val key = keyInput.text.toString().trim()
            val localAddr = localAddrInput.text.toString().trim()
            val remoteHost = remoteHostInput.text.toString().trim()
            val remotePort = remotePortInput.text.toString().trim()

            if (key.isEmpty() || localAddr.isEmpty() || remoteHost.isEmpty() || remotePort.isEmpty()) {
                statusText.text = "status: ERROR — all fields are required (全項目の入力が必要です)"
                startButton.isEnabled = true
                return@setOnClickListener
            }

            CoroutineScope(Dispatchers.Main).launch {
                val log = StringBuilder()
                statusText.text = "status: starting..."

                val args = if (isConnect) {
                    listOf(
                        "connect",
                        "--listen", localAddr,
                        "--remote", remoteHost,
                        "--remote-port", remotePort,
                        "--key", key,
                    )
                } else {
                    listOf(
                        "serve",
                        "--bind", localAddr,
                        "--target", remoteHost,
                        "--target-port", remotePort,
                        "--key", key,
                    )
                }
                log.appendLine("args: $args")

                val started = withContext(Dispatchers.IO) { startServerProcess(args, log) }
                if (!started) {
                    statusText.text = "status: FAILED to start (see log)"
                    logText.text = log.toString()
                    startButton.isEnabled = true
                    return@launch
                }

                val healthy = if (isConnect) {
                    withContext(Dispatchers.IO) { pollLocalListener(localAddr, log) }
                } else {
                    withContext(Dispatchers.IO) {
                        kotlinx.coroutines.delay(1500)
                        val alive = serverProcess?.isAlive == true
                        log.appendLine("serve mode: process alive after 1.5s = $alive (no active healthz-equivalent exists for `serve`)")
                        alive
                    }
                }

                statusText.text = if (healthy) {
                    "status: RUNNING (${if (isConnect) "connect" else "serve"})"
                } else {
                    "status: started, but health check failed (see log)"
                }
                logText.text = log.toString()
                stopButton.isEnabled = true
            }
        }

        stopButton.setOnClickListener {
            serverProcess?.destroy()
            serverProcess = null
            statusText.text = "status: stopped"
            startButton.isEnabled = true
            stopButton.isEnabled = false
        }
    }

    private fun binaryPath(): File = File(applicationInfo.nativeLibraryDir, "librslinkfusion.so")

    /** `generate-key`のように即座に終了し標準出力を1回だけ読めばよいコマンド用。 */
    private fun runOneShot(args: List<String>, log: StringBuilder): String? {
        return try {
            val binary = binaryPath()
            if (!binary.exists()) {
                log.appendLine("ERROR: native binary not found at ${binary.absolutePath}")
                return null
            }
            val pb = ProcessBuilder(listOf(binary.absolutePath) + args)
            pb.redirectErrorStream(true)
            val process = pb.start()
            val output = process.inputStream.bufferedReader().readText().trim()
            process.waitFor()
            log.appendLine("output: $output")
            output.ifEmpty { null }
        } catch (e: Exception) {
            log.appendLine("ERROR: $e")
            null
        }
    }

    private fun startServerProcess(args: List<String>, log: StringBuilder): Boolean {
        return try {
            val binary = binaryPath()
            log.appendLine("binary path: ${binary.absolutePath}")
            log.appendLine("binary exists: ${binary.exists()}")
            if (!binary.exists()) {
                log.appendLine("ERROR: native binary not found — was the app built with jniLibs populated by cargo ndk?")
                return false
            }

            val pb = ProcessBuilder(listOf(binary.absolutePath) + args)
            pb.directory(filesDir)
            pb.redirectErrorStream(true)
            val process = pb.start()
            serverProcess = process

            Thread {
                try {
                    BufferedReader(InputStreamReader(process.inputStream)).use { reader ->
                        var line: String?
                        while (reader.readLine().also { line = it } != null) {
                            android.util.Log.i("rs-linkfusion", line ?: "")
                        }
                    }
                } catch (_: Exception) {
                    // プロセス終了時にストリームが閉じるのは正常系。
                }
            }.start()

            log.appendLine("process started (alive=${process.isAlive})")
            true
        } catch (e: Exception) {
            log.appendLine("ERROR launching process: $e")
            false
        }
    }

    /**
     * `connect`モードのローカルリスンアドレスへ実際にTCP接続を試み、
     * 「ローカルの受け口が実際に開いていること」を確認する
     * (`/healthz`が無いこの実行ファイル向けの代替手段)。
     */
    private fun pollLocalListener(localAddr: String, log: StringBuilder): Boolean {
        val parts = localAddr.split(":")
        if (parts.size != 2) {
            log.appendLine("cannot parse local address for health check: $localAddr")
            return false
        }
        val host = parts[0]
        val port = parts[1].toIntOrNull() ?: return false

        repeat(10) { attempt ->
            try {
                Thread.sleep(300)
                Socket().use { socket ->
                    socket.connect(InetSocketAddress(host, port), 1000)
                    log.appendLine("attempt ${attempt + 1}: TCP connect to $localAddr succeeded")
                    return true
                }
            } catch (e: Exception) {
                log.appendLine("attempt ${attempt + 1}: TCP connect to $localAddr failed: ${e.message}")
            }
        }
        return false
    }

    override fun onDestroy() {
        super.onDestroy()
        networkBinder.stop()
        serverProcess?.destroy()
    }
}
