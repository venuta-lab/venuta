package io.github.venuta_lab.spike

import android.app.Activity
import android.app.ActivityManager
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.webkit.WebView
import android.widget.TextView
import kotlinx.coroutines.runBlocking

/** JNI side of the spike library (the UniFFI side goes through JNA). */
object Native {
    @JvmStatic external fun installContext(context: Context): Int
}

/**
 * Android spike (docs/android-spike.md): reads the device facts only an app can see, loads the
 * Rust core, makes one call of each kind, and closes. Intent extra `phase`: `full` (default) or
 * `restart` (only reads the vault back, after a force-stop).
 */
class MainActivity : Activity() {
    private val tag = "VENUTA_SPIKE"
    private lateinit var view: TextView

    private fun say(line: String) {
        Log.i(tag, line)
        runOnUiThread { view.append(line + "\n") }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        view = TextView(this).apply { textSize = 10f }
        setContentView(view)
        val phase = intent.getStringExtra("phase") ?: "full"
        Thread {
            try {
                if (phase == "restart") restart() else full()
            } catch (t: Throwable) {
                Log.e(tag, "FAILED", t)
                say("FAILED: $t")
            }
            say("DONE phase=$phase")
            if (intent.getBooleanExtra("stay", false).not()) runOnUiThread { finish() }
        }.start()
    }

    private fun full() {
        deviceFacts()
        val t0 = System.nanoTime()
        System.loadLibrary("venuta_spike")
        say("loadLibrary ok in ${(System.nanoTime() - t0) / 1_000_000} ms")
        val t1 = System.nanoTime()
        say("core_info: ${uniffi.venuta_spike.coreInfo()}")
        say("first UniFFI call (JNA load + checksums) ${(System.nanoTime() - t1) / 1_000_000} ms")
        say("init_paths: ${uniffi.venuta_spike.initPaths(filesDir.absolutePath, cacheDir.absolutePath)}")
        say("core_info after: ${uniffi.venuta_spike.coreInfo()}")
        say("decode_smoke: ${uniffi.venuta_spike.decodeSmoke(cacheDir.absolutePath)}")
        say("async_echo: ${runBlocking { uniffi.venuta_spike.asyncEcho(50uL) }}")
        say("link_everything(false): ${uniffi.venuta_spike.linkEverything(false)}")
        say("vault: ${uniffi.venuta_spike.vaultPutGet("spike-key", "kept-${System.currentTimeMillis()}")}")
        say("iroh without context: ${runBlocking { uniffi.venuta_spike.irohProbe("no-context") }}")
        say("installContext: ${Native.installContext(applicationContext)}")
        say("iroh with context: ${runBlocking { uniffi.venuta_spike.irohProbe("with-context") }}")
        say("panics left: ${uniffi.venuta_spike.takePanics()}")
    }

    private fun restart() {
        System.loadLibrary("venuta_spike")
        say("vault after restart: ${uniffi.venuta_spike.vaultGet("spike-key")}")
    }

    private fun deviceFacts() {
        val am = getSystemService(AudioManager::class.java)
        say("audio: outputSampleRate=${am.getProperty(AudioManager.PROPERTY_OUTPUT_SAMPLE_RATE)} framesPerBuffer=${am.getProperty(AudioManager.PROPERTY_OUTPUT_FRAMES_PER_BUFFER)} lowLatency=${packageManager.hasSystemFeature(PackageManager.FEATURE_AUDIO_LOW_LATENCY)} pro=${packageManager.hasSystemFeature(PackageManager.FEATURE_AUDIO_PRO)} output=${packageManager.hasSystemFeature(PackageManager.FEATURE_AUDIO_OUTPUT)}")
        val outs = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS).joinToString { "${it.type}:${it.productName}:rates=${it.sampleRates.toList()}" }
        say("audio outputs: $outs")
        val attrs = AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build()
        val enc = mapOf(
            "MP3" to AudioFormat.ENCODING_MP3,
            "AAC_LC" to AudioFormat.ENCODING_AAC_LC,
            "OPUS" to AudioFormat.ENCODING_OPUS,
            "PCM16" to AudioFormat.ENCODING_PCM_16BIT,
            "PCM_FLOAT" to AudioFormat.ENCODING_PCM_FLOAT,
        )
        val offload = enc.map { (n, e) ->
            val f = AudioFormat.Builder().setEncoding(e).setSampleRate(44100).setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).build()
            "$n=${runCatching { AudioManager.isOffloadedPlaybackSupported(f, attrs) }.getOrElse { "err" }}"
        }
        say("offload (speaker route now, 44.1k stereo): ${offload.joinToString()}")

        val act = getSystemService(ActivityManager::class.java)
        val mi = ActivityManager.MemoryInfo().also { act.getMemoryInfo(it) }
        say("memory: isLowRamDevice=${act.isLowRamDevice} memoryClass=${act.memoryClass} largeMemoryClass=${act.largeMemoryClass} totalMem=${mi.totalMem / 1048576} MB availMem=${mi.availMem / 1048576} MB threshold=${mi.threshold / 1048576} MB")

        val dm = resources.displayMetrics
        val c = resources.configuration
        say("display: ${dm.widthPixels}x${dm.heightPixels} px densityDpi=${dm.densityDpi} density=${dm.density} screenDp=${c.screenWidthDp}x${c.screenHeightDp} smallestDp=${c.smallestScreenWidthDp}")
        say("webview: ${WebView.getCurrentWebViewPackage()?.let { "${it.packageName} ${it.versionName}" }} sdk=${Build.VERSION.SDK_INT}")

        for (id in InputDevice.getDeviceIds()) {
            val d = InputDevice.getDevice(id) ?: continue
            val s = d.sources
            val kinds = listOf(
                "GAMEPAD" to InputDevice.SOURCE_GAMEPAD,
                "DPAD" to InputDevice.SOURCE_DPAD,
                "JOYSTICK" to InputDevice.SOURCE_JOYSTICK,
                "KEYBOARD" to InputDevice.SOURCE_KEYBOARD,
                "TOUCHSCREEN" to InputDevice.SOURCE_TOUCHSCREEN,
                "MOUSE" to InputDevice.SOURCE_MOUSE,
            ).filter { (_, v) -> s and v == v }.joinToString("|") { it.first }
            val axes = d.motionRanges.joinToString { MotionEvent.axisToString(it.axis) }
            val keys = intArrayOf(
                KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN, KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT,
                KeyEvent.KEYCODE_BUTTON_A, KeyEvent.KEYCODE_BUTTON_B, KeyEvent.KEYCODE_BUTTON_X, KeyEvent.KEYCODE_BUTTON_Y,
                KeyEvent.KEYCODE_BUTTON_L1, KeyEvent.KEYCODE_BUTTON_R1, KeyEvent.KEYCODE_BUTTON_L2, KeyEvent.KEYCODE_BUTTON_R2,
                KeyEvent.KEYCODE_BUTTON_START, KeyEvent.KEYCODE_BUTTON_SELECT, KeyEvent.KEYCODE_BUTTON_MODE, KeyEvent.KEYCODE_BACK,
                KeyEvent.KEYCODE_BUTTON_THUMBL, KeyEvent.KEYCODE_BUTTON_THUMBR, KeyEvent.KEYCODE_BUTTON_C, KeyEvent.KEYCODE_BUTTON_Z,
            )
            val has = d.hasKeys(*keys)
            val keyList = keys.indices.filter { has[it] }.joinToString(",") { KeyEvent.keyCodeToString(keys[it]).removePrefix("KEYCODE_") }
            say("input #$id '${d.name}' sources=0x${Integer.toHexString(s)} [$kinds] virtual=${d.isVirtual} external=${d.isExternal} controller=${d.controllerNumber} vendor=0x${Integer.toHexString(d.vendorId)} product=0x${Integer.toHexString(d.productId)} axes=[$axes] keys=[$keyList]")
        }
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        Log.i(tag, "key ${KeyEvent.keyCodeToString(keyCode)} scan=${event.scanCode} source=0x${Integer.toHexString(event.source)} device=${event.device?.name}")
        return super.onKeyDown(keyCode, event)
    }

    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        Log.i(tag, "motion hat=${event.getAxisValue(MotionEvent.AXIS_HAT_X)},${event.getAxisValue(MotionEvent.AXIS_HAT_Y)} source=0x${Integer.toHexString(event.source)}")
        return super.onGenericMotionEvent(event)
    }
}
