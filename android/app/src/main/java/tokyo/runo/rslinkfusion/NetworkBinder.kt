package tokyo.runo.rslinkfusion

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest

/**
 * WiFi回線とUSB-Ethernetアダプタ回線の両方を同時に保持するためのヘルパー
 * (2026-08-05新設、ユーザー要望「WiFi回線とUSB有線LANアダプタの2回線を
 * 同時にボンディングし、どちらか一方が切断されても通信を継続できるように
 * したい」への対応)。
 *
 * 日英Web検索で裏取り済みの標準Android API:
 * `ConnectivityManager.requestNetwork(NetworkRequest, NetworkCallback)`を
 * `TRANSPORT_WIFI`・`TRANSPORT_ETHERNET`それぞれに対して呼ぶと、Androidの
 * デフォルトルーティング(通常はWiFiのみがアクティブなデフォルトネット
 * ワークになる)とは別に、明示的にそのトランスポート種別のネットワークを
 * 要求・保持できる(複数の`NetworkRequest`を同時に保持可能)。
 *
 * **正直な開示・既知の制約(このシンプルな実装が意図的にやらないこと)**:
 * このクラスは「WiFi/USB-Ethernetの両方が現在利用可能か」を検知・保持し、
 * `Network`オブジェクトと`ConnectivityManager.getLinkProperties(network)`
 * 経由で実際のインターフェース名(例: `wlan0`/`eth0`)を取得するところまでを
 * 行う。しかし`rs-linkfusion`本体は**別プロセス(ProcessBuilderで起動した
 * ネイティブバイナリ)**であり、`Network.bindSocket()`はこのJVMプロセス内で
 * 生成したソケットにしか効かない——別プロセスの子ソケットへ直接
 * 適用することはできない。したがって今回の実装は「両ネットワークが実際に
 * 存在し、そのインターフェース名が何か」をログ・画面表示で確認できる
 * ところまでに留め、過剰設計(プロセス間でソケットFDを受け渡す・
 * VpnServiceでラップする等)は行わない、というシンプルな方針を採った。
 * `rs-linkfusion`本体側は`aggligator-transport-tcp`が
 * `NetworkInterface::show()`(今回パッチした`vendor/network-interface`
 * 経由)で列挙した全インターフェース名に対して個別にボンディングリンクを
 * 張る設計のため、OSレベルで両インターフェースが有効になってさえいれば、
 * 本体側が自動的に両方を使う(既存の`multi_interface`の既定動作、
 * README/CLAUDE.md参照)。
 */
class NetworkBinder(context: Context) {
    private val connectivityManager =
        context.applicationContext.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager

    data class NetworkStatus(
        val wifiAvailable: Boolean,
        val wifiInterfaceName: String?,
        val ethernetAvailable: Boolean,
        val ethernetInterfaceName: String?,
    )

    @Volatile
    private var wifiNetwork: Network? = null

    @Volatile
    private var ethernetNetwork: Network? = null

    private var wifiCallback: ConnectivityManager.NetworkCallback? = null
    private var ethernetCallback: ConnectivityManager.NetworkCallback? = null

    /** 両トランスポートの`NetworkRequest`を発行し、以後の変化を[onChange]へ通知する。 */
    fun start(onChange: (NetworkStatus) -> Unit) {
        fun notify() = onChange(currentStatus())

        val wifiRequest = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
            .build()
        val wifiCb = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                wifiNetwork = network
                notify()
            }

            override fun onLost(network: Network) {
                if (wifiNetwork == network) wifiNetwork = null
                notify()
            }
        }
        wifiCallback = wifiCb

        val ethernetRequest = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_ETHERNET)
            .build()
        val ethernetCb = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                ethernetNetwork = network
                notify()
            }

            override fun onLost(network: Network) {
                if (ethernetNetwork == network) ethernetNetwork = null
                notify()
            }
        }
        ethernetCallback = ethernetCb

        try {
            connectivityManager.requestNetwork(wifiRequest, wifiCb)
            connectivityManager.requestNetwork(ethernetRequest, ethernetCb)
        } catch (e: Exception) {
            // 一部端末/OSバージョンで同時多重requestNetworkが制限される
            // ケースへの安全策(既存の他android版と同じ「例外を握りつぶし
            // 機能低下のみに留める」方針)。
        }
        notify()
    }

    fun stop() {
        wifiCallback?.let { try { connectivityManager.unregisterNetworkCallback(it) } catch (_: Exception) {} }
        ethernetCallback?.let { try { connectivityManager.unregisterNetworkCallback(it) } catch (_: Exception) {} }
        wifiCallback = null
        ethernetCallback = null
    }

    private fun interfaceNameOf(network: Network?): String? {
        if (network == null) return null
        return try {
            connectivityManager.getLinkProperties(network)?.interfaceName
        } catch (e: Exception) {
            null
        }
    }

    fun currentStatus(): NetworkStatus = NetworkStatus(
        wifiAvailable = wifiNetwork != null,
        wifiInterfaceName = interfaceNameOf(wifiNetwork),
        ethernetAvailable = ethernetNetwork != null,
        ethernetInterfaceName = interfaceNameOf(ethernetNetwork),
    )
}
